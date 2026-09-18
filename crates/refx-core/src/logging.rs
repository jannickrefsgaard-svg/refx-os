//! Structured logging via `tracing`.

use tracing_subscriber::EnvFilter;

use crate::config::LoggingConfig;

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("invalid log filter: {0}")]
    Filter(String),
    #[error("global logger already initialized")]
    AlreadyInitialized,
}

/// Install the global logger. Call once, early in the host process.
/// Libraries (including REFX Core) never call this themselves.
pub fn init(cfg: &LoggingConfig) -> Result<(), LoggingError> {
    let filter = EnvFilter::try_new(&cfg.level).map_err(|e| LoggingError::Filter(e.to_string()))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(cfg.ansi)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInitialized)
}
