//! Integration tests: runtime + services + event bus + tasks together.

use std::sync::{Arc, Mutex};

use refx_core::event::Event;
use refx_core::runtime::{RuntimeError, ServiceError};
use refx_core::{Priority, RefxConfig, Runtime, RuntimeState, Service, ServiceContext, TaskState};

type Log = Arc<Mutex<Vec<String>>>;

#[derive(Debug)]
struct Probe {
    name: String,
    log: Log,
    fail_start: bool,
    fail_stop: bool,
}

impl Probe {
    fn boxed(name: &str, log: &Log) -> Box<Self> {
        Box::new(Self {
            name: name.into(),
            log: Arc::clone(log),
            fail_start: false,
            fail_stop: false,
        })
    }
}

impl Service for Probe {
    fn name(&self) -> &str {
        &self.name
    }
    fn start(&mut self, _: &ServiceContext) -> Result<(), ServiceError> {
        if self.fail_start {
            return Err("boom".into());
        }
        self.log.lock().expect("log").push(format!("start {}", self.name));
        Ok(())
    }
    fn stop(&mut self) -> Result<(), ServiceError> {
        self.log.lock().expect("log").push(format!("stop {}", self.name));
        if self.fail_stop {
            return Err("stuck".into());
        }
        Ok(())
    }
}

fn log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

#[test]
fn services_start_in_order_and_stop_in_reverse() {
    let l = log();
    let mut rt = Runtime::new(RefxConfig::default());
    for n in ["a", "b", "c"] {
        rt.register(Probe::boxed(n, &l)).expect("register");
    }
    let sub = rt.context().bus.subscribe();
    rt.start().expect("start");
    assert_eq!(rt.state(), RuntimeState::Running);
    rt.shutdown().expect("shutdown");
    assert_eq!(rt.state(), RuntimeState::Stopped);

    assert_eq!(
        *l.lock().expect("log"),
        ["start a", "start b", "start c", "stop c", "stop b", "stop a"]
    );
    let events: Vec<_> = sub.drain().into_iter().map(|e| e.event).collect();
    assert_eq!(events[3], Event::RuntimeStarted);
    assert_eq!(events.last(), Some(&Event::RuntimeStopped));
}

#[test]
fn failed_start_rolls_back_started_services() {
    let l = log();
    let mut rt = Runtime::new(RefxConfig::default());
    rt.register(Probe::boxed("a", &l)).expect("register");
    let mut bad = Probe::boxed("b", &l);
    bad.fail_start = true;
    rt.register(bad).expect("register");
    rt.register(Probe::boxed("c", &l)).expect("register");

    let err = rt.start().expect_err("start must fail");
    assert!(matches!(err, RuntimeError::StartFailed { ref name, .. } if name == "b"));
    assert_eq!(rt.state(), RuntimeState::Stopped);
    assert_eq!(*l.lock().expect("log"), ["start a", "stop a"], "c never started, a rolled back");
}

#[test]
fn stop_failure_still_stops_remaining_services() {
    let l = log();
    let mut rt = Runtime::new(RefxConfig::default());
    rt.register(Probe::boxed("a", &l)).expect("register");
    let mut stuck = Probe::boxed("b", &l);
    stuck.fail_stop = true;
    rt.register(stuck).expect("register");
    rt.start().expect("start");

    let err = rt.shutdown().expect_err("shutdown must report failure");
    assert!(matches!(err, RuntimeError::StopFailed(ref f) if f.len() == 1));
    assert_eq!(rt.state(), RuntimeState::Stopped);
    assert!(l.lock().expect("log").contains(&"stop a".to_string()));
}

#[test]
fn lifecycle_misuse_is_rejected() {
    let l = log();
    let mut rt = Runtime::new(RefxConfig::default());
    assert!(matches!(rt.shutdown(), Err(RuntimeError::NotRunning)));
    rt.register(Probe::boxed("a", &l)).expect("register");
    assert!(matches!(rt.register(Probe::boxed("a", &l)), Err(RuntimeError::DuplicateService(_))));
    rt.start().expect("start");
    assert!(matches!(rt.start(), Err(RuntimeError::AlreadyRunning)));
    assert!(matches!(rt.register(Probe::boxed("z", &l)), Err(RuntimeError::RegisterWhileRunning)));
}

#[test]
fn dropping_running_runtime_stops_services() {
    let l = log();
    {
        let mut rt = Runtime::new(RefxConfig::default());
        rt.register(Probe::boxed("a", &l)).expect("register");
        rt.start().expect("start");
    }
    assert_eq!(*l.lock().expect("log"), ["start a", "stop a"]);
}

#[test]
fn tasks_are_available_through_runtime_context() {
    let rt = Runtime::new(RefxConfig::default());
    let tasks = &rt.context().tasks;
    let id = tasks.create("Summarize this folder.", Priority::Normal).expect("create");
    tasks.transition(id, TaskState::Queued, None).expect("queue");
    assert_eq!(tasks.active().len(), 1);
}
