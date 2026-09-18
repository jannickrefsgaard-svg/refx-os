//! Event bus (Master Specification §6).
//!
//! Components communicate through events rather than direct coupling.
//! Phase 0 uses a synchronous fan-out over `std::sync::mpsc` channels:
//! every subscriber receives every event, in publish order.
//! See `docs/decisions/0002-event-bus.md` for why no async runtime yet.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::task::{TaskId, TaskState};

/// A system event. Variant names follow §6 of the master specification.
///
/// Variants for subsystems that do not exist yet (agents, models, voice,
/// permissions, applications) are declared now so that the event vocabulary
/// is stable, but nothing emits them in Phase 0.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    // Runtime / services
    RuntimeStarted,
    RuntimeStopping,
    RuntimeStopped,
    ServiceStarted { name: String },
    ServiceStopped { name: String },
    ServiceFailed { name: String, error: String },

    // Tasks
    TaskCreated { id: TaskId },
    TaskStateChanged { id: TaskId, from: TaskState, to: TaskState },
    TaskCompleted { id: TaskId },

    // Reserved for later phases
    AgentStarted { agent: String },
    AgentStopped { agent: String },
    ModelLoaded { model: String },
    ModelUnloaded { model: String },
    VoiceDetected,
    PermissionRequested { subject: String, capability: String },
    PermissionDenied { subject: String, capability: String },
    ApplicationOpened { app: String },
    ApplicationClosed { app: String },
    SystemResourceChanged { resource: String },
}

/// An event plus delivery metadata, so every event is traceable (§2.7, §34).
#[derive(Debug, Clone)]
pub struct EventEnvelope {
    /// Monotonic sequence number, unique per bus.
    pub seq: u64,
    pub timestamp: SystemTime,
    /// Component that published the event (e.g. `"core.tasks"`).
    pub source: String,
    pub event: Event,
}

/// Fan-out event bus. Cheap to share via `Arc<EventBus>`.
#[derive(Debug, Default)]
pub struct EventBus {
    next_seq: AtomicU64,
    subscribers: Mutex<Vec<Sender<EventEnvelope>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new subscriber. It receives only events published after this call.
    pub fn subscribe(&self) -> Subscription {
        let (tx, rx) = mpsc::channel();
        self.lock_subscribers().push(tx);
        Subscription { rx }
    }

    /// Publish an event to every live subscriber. Dropped subscribers are pruned.
    /// Returns the sequence number assigned to the event.
    pub fn publish(&self, source: &str, event: Event) -> u64 {
        let mut subs = self.lock_subscribers();
        // Sequence is assigned under the lock so delivery order == seq order.
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let envelope =
            EventEnvelope { seq, timestamp: SystemTime::now(), source: source.to_owned(), event };
        tracing::trace!(seq, source, event = ?envelope.event, "event published");
        subs.retain(|tx| tx.send(envelope.clone()).is_ok());
        seq
    }

    /// Number of currently connected subscribers (after the last prune).
    pub fn subscriber_count(&self) -> usize {
        self.lock_subscribers().len()
    }

    fn lock_subscribers(&self) -> std::sync::MutexGuard<'_, Vec<Sender<EventEnvelope>>> {
        // A poisoned lock only means another thread panicked mid-publish;
        // the Vec itself is still structurally valid, so recover it.
        self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Receiving end of a bus subscription. Dropping it unsubscribes.
#[derive(Debug)]
pub struct Subscription {
    rx: Receiver<EventEnvelope>,
}

impl Subscription {
    /// Non-blocking receive.
    pub fn try_recv(&self) -> Option<EventEnvelope> {
        self.rx.try_recv().ok()
    }

    /// Blocking receive with timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<EventEnvelope> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Drain all currently queued events.
    pub fn drain(&self) -> Vec<EventEnvelope> {
        std::iter::from_fn(|| self.try_recv()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn every_subscriber_receives_every_event_in_order() {
        let bus = EventBus::new();
        let a = bus.subscribe();
        let b = bus.subscribe();
        bus.publish("test", Event::RuntimeStarted);
        bus.publish("test", Event::RuntimeStopped);

        for sub in [&a, &b] {
            let got: Vec<_> = sub.drain().into_iter().map(|e| (e.seq, e.event)).collect();
            assert_eq!(got, vec![(0, Event::RuntimeStarted), (1, Event::RuntimeStopped)]);
        }
    }

    #[test]
    fn late_subscriber_does_not_see_earlier_events() {
        let bus = EventBus::new();
        bus.publish("test", Event::RuntimeStarted);
        let late = bus.subscribe();
        assert!(late.try_recv().is_none());
    }

    #[test]
    fn dropped_subscribers_are_pruned() {
        let bus = EventBus::new();
        let keep = bus.subscribe();
        drop(bus.subscribe());
        assert_eq!(bus.subscriber_count(), 2);
        bus.publish("test", Event::VoiceDetected);
        assert_eq!(bus.subscriber_count(), 1);
        assert!(keep.try_recv().is_some());
    }

    #[test]
    fn envelope_records_source() {
        let bus = EventBus::new();
        let sub = bus.subscribe();
        bus.publish("core.tasks", Event::RuntimeStarted);
        assert_eq!(sub.try_recv().map(|e| e.source).as_deref(), Some("core.tasks"));
    }

    #[test]
    fn concurrent_publishers_produce_unique_ordered_sequence() {
        let bus = Arc::new(EventBus::new());
        let sub = bus.subscribe();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let bus = Arc::clone(&bus);
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        bus.publish("t", Event::VoiceDetected);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("publisher thread panicked");
        }
        let seqs: Vec<u64> = sub.drain().into_iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (0..800).collect::<Vec<_>>());
    }
}
