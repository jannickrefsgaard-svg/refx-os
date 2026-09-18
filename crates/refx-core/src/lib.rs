//! # REFX Core
//!
//! Foundation of the REFX platform (Master Specification §5). Core is
//! deliberately independent of any AI model, provider or host platform.
//!
//! Phase 0 scope:
//! - [`event`]: in-process event bus (§6)
//! - [`task`]: task model, state machine and task manager (§7)
//! - [`config`]: typed, validated configuration
//! - [`logging`]: structured logging setup
//! - [`runtime`]: service lifecycle and core runtime

pub mod config;
pub mod event;
pub mod logging;
pub mod runtime;
pub mod task;

pub use config::RefxConfig;
pub use event::{Event, EventBus, EventEnvelope, Subscription};
pub use runtime::{Runtime, RuntimeState, Service, ServiceContext};
pub use task::{Priority, Task, TaskId, TaskManager, TaskState};
