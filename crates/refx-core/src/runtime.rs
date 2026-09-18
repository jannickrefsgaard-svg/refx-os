//! Core runtime and service lifecycle (Master Specification §5).
//!
//! Services start in registration order and stop in reverse order. If any
//! service fails to start, every already-started service is stopped again
//! and the runtime returns to `Stopped` — no half-started system.

use std::fmt;
use std::sync::Arc;

use crate::config::RefxConfig;
use crate::event::{Event, EventBus};
use crate::task::TaskManager;

const SOURCE: &str = "core.runtime";

/// Shared handles every service receives on start.
#[derive(Debug, Clone)]
pub struct ServiceContext {
    pub config: Arc<RefxConfig>,
    pub bus: Arc<EventBus>,
    pub tasks: Arc<TaskManager>,
}

pub type ServiceError = Box<dyn std::error::Error + Send + Sync>;

/// A long-lived subsystem managed by the runtime.
pub trait Service: Send + fmt::Debug {
    /// Stable identifier, e.g. `"core.scheduler"`.
    fn name(&self) -> &str;
    fn start(&mut self, ctx: &ServiceContext) -> Result<(), ServiceError>;
    fn stop(&mut self) -> Result<(), ServiceError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    Stopped,
    Running,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime is already running")]
    AlreadyRunning,
    #[error("runtime is not running")]
    NotRunning,
    #[error("service name `{0}` is already registered")]
    DuplicateService(String),
    #[error("cannot register services while running")]
    RegisterWhileRunning,
    #[error("service `{name}` failed to start: {source}")]
    StartFailed { name: String, source: ServiceError },
    #[error("{} service(s) failed to stop cleanly: {}", .0.len(), .0.join("; "))]
    StopFailed(Vec<String>),
}

#[derive(Debug)]
pub struct Runtime {
    ctx: ServiceContext,
    services: Vec<Box<dyn Service>>,
    started: usize,
    state: RuntimeState,
}

impl Runtime {
    pub fn new(config: RefxConfig) -> Self {
        let bus = Arc::new(EventBus::new());
        let tasks = Arc::new(TaskManager::new(Arc::clone(&bus)));
        Self {
            ctx: ServiceContext { config: Arc::new(config), bus, tasks },
            services: Vec::new(),
            started: 0,
            state: RuntimeState::Stopped,
        }
    }

    pub fn context(&self) -> &ServiceContext {
        &self.ctx
    }

    pub fn state(&self) -> RuntimeState {
        self.state
    }

    pub fn service_names(&self) -> Vec<&str> {
        self.services.iter().map(|s| s.name()).collect()
    }

    pub fn register(&mut self, service: Box<dyn Service>) -> Result<(), RuntimeError> {
        if self.state == RuntimeState::Running {
            return Err(RuntimeError::RegisterWhileRunning);
        }
        if self.services.iter().any(|s| s.name() == service.name()) {
            return Err(RuntimeError::DuplicateService(service.name().to_owned()));
        }
        self.services.push(service);
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), RuntimeError> {
        if self.state == RuntimeState::Running {
            return Err(RuntimeError::AlreadyRunning);
        }
        tracing::info!(services = self.services.len(), "starting REFX runtime");
        for i in 0..self.services.len() {
            let service = &mut self.services[i];
            let name = service.name().to_owned();
            match service.start(&self.ctx) {
                Ok(()) => {
                    self.started = i + 1;
                    tracing::info!(service = %name, "service started");
                    self.ctx.bus.publish(SOURCE, Event::ServiceStarted { name });
                }
                Err(source) => {
                    tracing::error!(service = %name, error = %source, "service failed to start; rolling back");
                    self.ctx.bus.publish(
                        SOURCE,
                        Event::ServiceFailed { name: name.clone(), error: source.to_string() },
                    );
                    // Rollback errors are logged by stop_started; the start
                    // failure is the error we report.
                    let _ = self.stop_started();
                    return Err(RuntimeError::StartFailed { name, source });
                }
            }
        }
        self.state = RuntimeState::Running;
        self.ctx.bus.publish(SOURCE, Event::RuntimeStarted);
        Ok(())
    }

    /// Stop all services in reverse order. Every service gets a stop call
    /// even if an earlier one fails; all failures are reported together.
    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if self.state != RuntimeState::Running {
            return Err(RuntimeError::NotRunning);
        }
        tracing::info!("stopping REFX runtime");
        self.ctx.bus.publish(SOURCE, Event::RuntimeStopping);
        let result = self.stop_started();
        self.state = RuntimeState::Stopped;
        self.ctx.bus.publish(SOURCE, Event::RuntimeStopped);
        result
    }

    fn stop_started(&mut self) -> Result<(), RuntimeError> {
        let mut failures = Vec::new();
        for service in self.services[..self.started].iter_mut().rev() {
            let name = service.name().to_owned();
            match service.stop() {
                Ok(()) => {
                    tracing::info!(service = %name, "service stopped");
                    self.ctx.bus.publish(SOURCE, Event::ServiceStopped { name });
                }
                Err(e) => {
                    tracing::error!(service = %name, error = %e, "service failed to stop");
                    self.ctx.bus.publish(
                        SOURCE,
                        Event::ServiceFailed { name: name.clone(), error: e.to_string() },
                    );
                    failures.push(format!("{name}: {e}"));
                }
            }
        }
        self.started = 0;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::StopFailed(failures))
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if self.state == RuntimeState::Running {
            tracing::warn!("runtime dropped while running; shutting down");
            let _ = self.shutdown();
        }
    }
}
