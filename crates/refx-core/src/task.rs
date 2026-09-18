//! Task system (Master Specification §7).
//!
//! Every REFX action is represented as a [`Task`] with an explicit,
//! validated state machine. Invalid transitions are rejected rather than
//! silently accepted, so the system can never "pretend success" (§35).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::event::{Event, EventBus};

const SOURCE: &str = "core.tasks";

/// Task identifier. Unique within one runtime instance.
///
/// Phase 0 uses a monotonic counter; persistent, globally unique IDs are
/// introduced together with SQLite persistence (see ADR 0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Task states, exactly as listed in §7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskState {
    Created,
    Queued,
    Planning,
    Executing,
    Waiting,
    Paused,
    Failed,
    Cancelled,
    Completed,
}

impl TaskState {
    /// Terminal states accept no further transitions.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::Completed)
    }

    /// Allowed transitions. The spec lists the states but not the edges;
    /// this table is the Phase 0 design documented in ADR 0003.
    pub fn can_transition_to(self, to: TaskState) -> bool {
        use TaskState::*;
        if to == Cancelled {
            return !self.is_terminal();
        }
        matches!(
            (self, to),
            (Created, Queued)
                | (Queued, Planning | Executing)
                | (Planning, Executing | Waiting | Failed)
                | (Executing, Waiting | Paused | Failed | Completed)
                | (Waiting, Planning | Executing | Failed)
                | (Paused, Queued | Executing)
        )
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Created => "CREATED",
            Self::Queued => "QUEUED",
            Self::Planning => "PLANNING",
            Self::Executing => "EXECUTING",
            Self::Waiting => "WAITING",
            Self::Paused => "PAUSED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Completed => "COMPLETED",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

/// One recorded state change, kept for observability (§34).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRecord {
    pub from: TaskState,
    pub to: TaskState,
    pub at: SystemTime,
    pub reason: Option<String>,
}

/// Task record (§7). Agent/model/tools/permissions are plain strings until
/// those subsystems exist (Phases 4–7); they are recorded, not enforced.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub user_request: String,
    pub intent: Option<String>,
    pub priority: Priority,
    pub state: TaskState,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub permissions: Vec<String>,
    pub created: SystemTime,
    pub started: Option<SystemTime>,
    pub completed: Option<SystemTime>,
    pub history: Vec<TransitionRecord>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskError {
    #[error("task {0} not found")]
    NotFound(TaskId),
    #[error("task {id}: invalid transition {from} -> {to}")]
    InvalidTransition { id: TaskId, from: TaskState, to: TaskState },
    #[error("task request must not be empty")]
    EmptyRequest,
}

/// Owns all live tasks and publishes task events on the bus.
#[derive(Debug)]
pub struct TaskManager {
    bus: Arc<EventBus>,
    next_id: AtomicU64,
    tasks: Mutex<BTreeMap<TaskId, Task>>,
}

impl TaskManager {
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self { bus, next_id: AtomicU64::new(1), tasks: Mutex::new(BTreeMap::new()) }
    }

    /// Create a task in state `CREATED`.
    pub fn create(&self, user_request: &str, priority: Priority) -> Result<TaskId, TaskError> {
        let request = user_request.trim();
        if request.is_empty() {
            return Err(TaskError::EmptyRequest);
        }
        let id = TaskId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let task = Task {
            id,
            user_request: request.to_owned(),
            intent: None,
            priority,
            state: TaskState::Created,
            agent: None,
            model: None,
            tools: Vec::new(),
            permissions: Vec::new(),
            created: SystemTime::now(),
            started: None,
            completed: None,
            history: Vec::new(),
        };
        self.lock().insert(id, task);
        tracing::info!(task = %id, ?priority, "task created");
        self.bus.publish(SOURCE, Event::TaskCreated { id });
        Ok(id)
    }

    /// Move a task to a new state, validating the transition.
    pub fn transition(
        &self,
        id: TaskId,
        to: TaskState,
        reason: Option<&str>,
    ) -> Result<(), TaskError> {
        let from = {
            let mut tasks = self.lock();
            let task = tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
            let from = task.state;
            if !from.can_transition_to(to) {
                tracing::warn!(task = %id, %from, %to, "rejected invalid task transition");
                return Err(TaskError::InvalidTransition { id, from, to });
            }
            let now = SystemTime::now();
            if to == TaskState::Executing && task.started.is_none() {
                task.started = Some(now);
            }
            if to.is_terminal() {
                task.completed = Some(now);
            }
            task.state = to;
            task.history.push(TransitionRecord {
                from,
                to,
                at: now,
                reason: reason.map(str::to_owned),
            });
            from
        }; // lock released before publishing
        tracing::info!(task = %id, %from, %to, reason, "task transition");
        self.bus.publish(SOURCE, Event::TaskStateChanged { id, from, to });
        if to == TaskState::Completed {
            self.bus.publish(SOURCE, Event::TaskCompleted { id });
        }
        Ok(())
    }

    /// Snapshot of a task.
    pub fn get(&self, id: TaskId) -> Option<Task> {
        self.lock().get(&id).cloned()
    }

    /// Snapshot of all tasks, ordered by id.
    pub fn list(&self) -> Vec<Task> {
        self.lock().values().cloned().collect()
    }

    /// Tasks not in a terminal state.
    pub fn active(&self) -> Vec<Task> {
        self.lock().values().filter(|t| !t.state.is_terminal()).cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<TaskId, Task>> {
        self.tasks.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskState::*;

    const ALL: [TaskState; 9] =
        [Created, Queued, Planning, Executing, Waiting, Paused, Failed, Cancelled, Completed];

    fn manager() -> (Arc<EventBus>, TaskManager) {
        let bus = Arc::new(EventBus::new());
        (Arc::clone(&bus), TaskManager::new(bus))
    }

    #[test]
    fn terminal_states_have_no_outgoing_transitions() {
        for from in ALL.into_iter().filter(|s| s.is_terminal()) {
            for to in ALL {
                assert!(!from.can_transition_to(to), "{from} -> {to} must be rejected");
            }
        }
    }

    #[test]
    fn every_non_terminal_state_can_be_cancelled() {
        for from in ALL.into_iter().filter(|s| !s.is_terminal()) {
            assert!(from.can_transition_to(Cancelled), "{from} must be cancellable");
        }
    }

    #[test]
    fn cannot_complete_without_executing() {
        for from in [Created, Queued, Planning, Waiting, Paused] {
            assert!(!from.can_transition_to(Completed), "{from} -> COMPLETED must be rejected");
        }
    }

    #[test]
    fn happy_path_records_timestamps_history_and_events() {
        let (bus, tm) = manager();
        let sub = bus.subscribe();
        let id = tm.create("Fix the authentication bug.", Priority::High).expect("create");
        for s in [Queued, Planning, Executing, Completed] {
            tm.transition(id, s, None).expect("valid transition");
        }
        let t = tm.get(id).expect("task exists");
        assert_eq!(t.state, Completed);
        assert!(t.started.is_some() && t.completed.is_some());
        assert_eq!(t.history.len(), 4);

        let events: Vec<_> = sub.drain().into_iter().map(|e| e.event).collect();
        assert_eq!(events.first(), Some(&Event::TaskCreated { id }));
        assert_eq!(events.last(), Some(&Event::TaskCompleted { id }));
        assert_eq!(events.len(), 1 + 4 + 1);
    }

    #[test]
    fn invalid_transition_is_rejected_and_state_unchanged() {
        let (bus, tm) = manager();
        let id = tm.create("x", Priority::Normal).expect("create");
        let sub = bus.subscribe();
        let err = tm.transition(id, Completed, None).expect_err("must be rejected");
        assert_eq!(err, TaskError::InvalidTransition { id, from: Created, to: Completed });
        assert_eq!(tm.get(id).map(|t| t.state), Some(Created));
        assert!(sub.try_recv().is_none(), "no event for rejected transition");
    }

    #[test]
    fn unknown_task_and_empty_request_are_errors() {
        let (_, tm) = manager();
        assert_eq!(tm.transition(TaskId(99), Queued, None), Err(TaskError::NotFound(TaskId(99))));
        assert_eq!(tm.create("   ", Priority::Low), Err(TaskError::EmptyRequest));
    }

    #[test]
    fn active_excludes_terminal_tasks() {
        let (_, tm) = manager();
        let a = tm.create("a", Priority::Normal).expect("create");
        let b = tm.create("b", Priority::Normal).expect("create");
        tm.transition(b, Cancelled, Some("user cancelled")).expect("cancel");
        let active: Vec<_> = tm.active().into_iter().map(|t| t.id).collect();
        assert_eq!(active, vec![a]);
        assert_eq!(
            tm.get(b).map(|t| t.history[0].reason.clone()),
            Some(Some("user cancelled".into()))
        );
    }
}
