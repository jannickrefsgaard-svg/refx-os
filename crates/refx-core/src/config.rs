//! Typed, validated configuration.
//!
//! Source precedence (highest first): environment overrides → config file →
//! built-in defaults. Unknown keys are rejected so typos never pass silently.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Environment variable overriding `logging.level`.
pub const ENV_LOG_LEVEL: &str = "REFX_LOG";

const VALID_LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RefxConfig {
    pub general: GeneralConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    /// Human-readable name for this REFX instance.
    pub instance_name: String,
    /// Root directory for REFX data (workspaces, database, logs).
    /// Relative paths are resolved against the config file's directory.
    pub data_dir: PathBuf,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { instance_name: "REFX".into(), data_dir: PathBuf::from("data") }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Either a level (`info`) or a full filter directive (`info,refx_core=debug`).
    pub level: String,
    /// Colored terminal output.
    pub ansi: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { level: "info".into(), ansi: true }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config file {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("invalid config file {path}: {source}")]
    // Boxed: `toml::de::Error` is large and would bloat every
    // `Result<_, ConfigError>` (clippy::result_large_err).
    Parse { path: PathBuf, source: Box<toml::de::Error> },
    #[error("invalid config value `{key}`: {message}")]
    Invalid { key: &'static str, message: String },
}

/// Where the configuration came from; logged at startup for observability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Defaults,
    File(PathBuf),
}

impl RefxConfig {
    /// Parse and validate TOML text.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Load from `path`. A missing file yields defaults; any other I/O or
    /// parse error is returned. Environment overrides are applied, then the
    /// result is validated.
    pub fn load(path: &Path) -> Result<(Self, ConfigSource), ConfigError> {
        let (mut cfg, source) = match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut cfg = Self::from_toml_str(&text).map_err(|source| ConfigError::Parse {
                    path: path.to_owned(),
                    source: Box::new(source),
                })?;
                if cfg.general.data_dir.is_relative() {
                    if let Some(dir) = path.parent() {
                        cfg.general.data_dir = dir.join(&cfg.general.data_dir);
                    }
                }
                (cfg, ConfigSource::File(path.to_owned()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                (Self::default(), ConfigSource::Defaults)
            }
            Err(source) => return Err(ConfigError::Io { path: path.to_owned(), source }),
        };
        cfg.apply_env(|k| std::env::var(k).ok());
        cfg.validate()?;
        Ok((cfg, source))
    }

    /// Apply environment overrides. The lookup is injected for testability.
    pub fn apply_env(&mut self, lookup: impl Fn(&str) -> Option<String>) {
        if let Some(level) = lookup(ENV_LOG_LEVEL).filter(|v| !v.trim().is_empty()) {
            self.logging.level = level;
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.general.instance_name.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "general.instance_name",
                message: "must not be empty".into(),
            });
        }
        validate_log_filter(&self.logging.level)
    }
}

/// Accept `level` or comma-separated `target=level` directives.
fn validate_log_filter(filter: &str) -> Result<(), ConfigError> {
    let invalid = |message: String| ConfigError::Invalid { key: "logging.level", message };
    if filter.trim().is_empty() {
        return Err(invalid("must not be empty".into()));
    }
    for directive in filter.split(',').map(str::trim) {
        let level = directive.rsplit('=').next().unwrap_or(directive);
        if !VALID_LEVELS.contains(&level.to_ascii_lowercase().as_str()) {
            return Err(invalid(format!("`{level}` is not one of {}", VALID_LEVELS.join(", "))));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(RefxConfig::from_toml_str("").expect("parse"), RefxConfig::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let cfg = RefxConfig::from_toml_str("[logging]\nlevel = \"debug\"").expect("parse");
        assert_eq!(cfg.logging.level, "debug");
        assert!(cfg.logging.ansi);
        assert_eq!(cfg.general, GeneralConfig::default());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(RefxConfig::from_toml_str("[logging]\nlevle = \"debug\"").is_err());
        assert!(RefxConfig::from_toml_str("[nonsense]").is_err());
    }

    #[test]
    fn env_overrides_file() {
        let mut cfg = RefxConfig::default();
        cfg.apply_env(|k| (k == ENV_LOG_LEVEL).then(|| "trace".to_string()));
        assert_eq!(cfg.logging.level, "trace");
    }

    #[test]
    fn blank_env_value_is_ignored() {
        let mut cfg = RefxConfig::default();
        cfg.apply_env(|_| Some("  ".into()));
        assert_eq!(cfg.logging.level, "info");
    }

    #[test]
    fn log_filter_validation() {
        for ok in ["info", "WARN", "info,refx_core=debug", "refx_core::task=trace"] {
            assert!(validate_log_filter(ok).is_ok(), "{ok} should be valid");
        }
        for bad in ["", "verbose", "info,refx_core=loud"] {
            assert!(validate_log_filter(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn parse_error_keeps_message_and_source_chain() {
        use std::error::Error as _;
        let path = std::env::temp_dir()
            .join(format!("refx-config-parse-test-{}.toml", std::process::id()));
        std::fs::write(&path, "[logging]\nlevle = \"x\"\n").expect("write temp config");
        let err = RefxConfig::load(&path).expect_err("typo must be rejected");
        let _ = std::fs::remove_file(&path);

        assert!(matches!(err, ConfigError::Parse { .. }));
        let msg = err.to_string();
        assert!(msg.starts_with("invalid config file "), "{msg}");
        assert!(msg.contains("unknown field `levle`"), "{msg}");
        let source = err.source().expect("parse error exposes its source");
        assert!(source.to_string().contains("unknown field `levle`"));
    }

    #[test]
    fn empty_instance_name_is_invalid() {
        let mut cfg = RefxConfig::default();
        cfg.general.instance_name = " ".into();
        assert!(cfg.validate().is_err());
    }
}
