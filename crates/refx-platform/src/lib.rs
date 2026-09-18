//! # REFX Platform Interface (Master Specification §21)
//!
//! ```text
//! REFX Core -> Platform Interface -> Windows Adapter
//! ```
//!
//! REFX Core never talks to the host OS directly. Everything OS-specific
//! goes through [`Platform`]. Phase 0 ships only a portable adapter built on
//! `std`; the real Windows adapter (processes, apps, GPU, PowerShell…) is
//! Phase 2 work and will be added as another implementation of this trait.

use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),
    #[error("platform query failed: {0}")]
    Query(String),
}

/// Operating system family, as seen by REFX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsFamily {
    Windows,
    Linux,
    MacOs,
    Other,
}

impl OsFamily {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Self::Windows,
            "linux" => Self::Linux,
            "macos" => Self::MacOs,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for OsFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::MacOs => "macOS",
            Self::Other => "other",
        })
    }
}

/// Basic host information available in Phase 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub os: OsFamily,
    pub arch: &'static str,
    pub logical_cpus: usize,
}

/// The host-platform boundary. Implementations must be thread-safe.
pub trait Platform: Send + Sync + fmt::Debug {
    /// Adapter identifier, e.g. `"portable"` or `"windows"`.
    fn adapter_name(&self) -> &'static str;
    fn system_info(&self) -> Result<SystemInfo, PlatformError>;
}

/// Adapter using only the Rust standard library. Works everywhere, knows little.
#[derive(Debug, Default, Clone, Copy)]
pub struct PortablePlatform;

impl Platform for PortablePlatform {
    fn adapter_name(&self) -> &'static str {
        "portable"
    }

    fn system_info(&self) -> Result<SystemInfo, PlatformError> {
        let logical_cpus = std::thread::available_parallelism()
            .map_err(|e| PlatformError::Query(format!("cpu count: {e}")))?
            .get();
        Ok(SystemInfo { os: OsFamily::current(), arch: std::env::consts::ARCH, logical_cpus })
    }
}

/// Best available adapter for the current host.
pub fn detect() -> Box<dyn Platform> {
    // Phase 2: return the Windows adapter under cfg(windows).
    Box::new(PortablePlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_adapter_reports_sane_info() {
        let info = PortablePlatform.system_info().expect("system info");
        assert!(info.logical_cpus >= 1);
        assert_eq!(info.os, OsFamily::current());
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn detect_returns_an_adapter() {
        assert_eq!(detect().adapter_name(), "portable");
    }
}
