//! Typed application configuration loaded from environment variables.
//!
//! Per `rules.md` §13: env access is centralized here, never scattered as
//! `std::env::var(...)` calls throughout the codebase. All variables have
//! defaults so the app boots in a fresh checkout without an `.env` file.

use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::{AppError, AppResult};

/// Default Ollama base URL (OpenAI-compatible endpoint).
pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Default `SQLite` filename, resolved relative to the user data directory.
pub const DEFAULT_DB_FILENAME: &str = "testing-ide.db";

/// Default tracing filter directive.
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// Strongly-typed configuration assembled from the process environment.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Base URL for the Ollama OpenAI-compatible API.
    pub ollama_base_url: String,
    /// Absolute path to the `SQLite` database file.
    pub db_path: PathBuf,
    /// `tracing_subscriber::EnvFilter` directive, e.g. `info` or
    /// `testing_ide_lib=debug,sqlx=warn`.
    pub log_level: String,
}

impl AppConfig {
    /// Load configuration from the process environment, falling back to
    /// the documented defaults for any missing variable.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Config` if a value is present but cannot be
    /// parsed (e.g. an empty `DB_PATH`).
    pub fn from_env() -> AppResult<Self> {
        let ollama_base_url = read_string("OLLAMA_BASE_URL", DEFAULT_OLLAMA_BASE_URL)?;
        let db_path = read_path("DB_PATH", &default_db_path())?;
        let log_level = read_string("LOG_LEVEL", DEFAULT_LOG_LEVEL)?;

        Ok(Self {
            ollama_base_url,
            db_path,
            log_level,
        })
    }

    /// `sqlx`-compatible connection string for the configured `SQLite` file.
    ///
    /// Uses `?mode=rwc` so the file is created on first launch. The path
    /// is converted to a forward-slash string for cross-platform sqlite
    /// compatibility (Windows backslashes confuse the URI parser).
    #[must_use]
    pub fn database_url(&self) -> String {
        let path = self.db_path.display().to_string().replace('\\', "/");
        format!("sqlite://{path}?mode=rwc")
    }
}

fn read_string(key: &str, default: &str) -> AppResult<String> {
    match env::var(key) {
        Ok(value) if value.trim().is_empty() => Err(AppError::Config(format!(
            "environment variable {key} is set but empty"
        ))),
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_string()),
        Err(env::VarError::NotUnicode(_)) => Err(AppError::Config(format!(
            "environment variable {key} is not valid unicode"
        ))),
    }
}

fn read_path(key: &str, default: &Path) -> AppResult<PathBuf> {
    match env::var(key) {
        Ok(value) if value.trim().is_empty() => Err(AppError::Config(format!(
            "environment variable {key} is set but empty"
        ))),
        Ok(value) => PathBuf::from_str(&value).map_err(|e| AppError::Config(e.to_string())),
        Err(env::VarError::NotPresent) => Ok(default.to_path_buf()),
        Err(env::VarError::NotUnicode(_)) => Err(AppError::Config(format!(
            "environment variable {key} is not valid unicode"
        ))),
    }
}

/// Default DB path: `<cwd>/testing-ide.db`. The Tauri runtime overrides
/// this on first launch with the OS-specific user-data directory; this
/// fallback covers tests and CLI invocations.
fn default_db_path() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_DB_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard for env-var manipulation. Cargo runs tests in parallel by
    /// default; we serialize via a single mutex to avoid races on the
    /// process-global environment. `std::sync::Mutex` is sufficient — no
    /// new dep required (rules.md §11).
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn clear_vars() {
        env::remove_var("OLLAMA_BASE_URL");
        env::remove_var("DB_PATH");
        env::remove_var("LOG_LEVEL");
    }

    #[test]
    fn defaults_apply_when_env_unset() {
        let _g = env_guard();
        clear_vars();

        let cfg = AppConfig::from_env().expect("defaults must always load");
        assert_eq!(cfg.ollama_base_url, DEFAULT_OLLAMA_BASE_URL);
        assert_eq!(cfg.log_level, DEFAULT_LOG_LEVEL);
        assert!(cfg.db_path.ends_with(DEFAULT_DB_FILENAME));
    }

    #[test]
    fn env_overrides_defaults() {
        let _g = env_guard();
        clear_vars();
        env::set_var("OLLAMA_BASE_URL", "http://example.test:11434");
        env::set_var("LOG_LEVEL", "debug");

        let cfg = AppConfig::from_env().expect("override must succeed");
        assert_eq!(cfg.ollama_base_url, "http://example.test:11434");
        assert_eq!(cfg.log_level, "debug");

        clear_vars();
    }

    #[test]
    fn empty_env_value_is_rejected() {
        let _g = env_guard();
        clear_vars();
        env::set_var("OLLAMA_BASE_URL", "");

        let err = AppConfig::from_env().expect_err("empty value must error");
        assert_eq!(err.code(), "CONFIG_ERROR");

        clear_vars();
    }

    #[test]
    fn database_url_uses_forward_slashes() {
        let cfg = AppConfig {
            ollama_base_url: DEFAULT_OLLAMA_BASE_URL.into(),
            db_path: PathBuf::from(r"C:\data\test.db"),
            log_level: DEFAULT_LOG_LEVEL.into(),
        };
        let url = cfg.database_url();
        assert!(url.starts_with("sqlite://"));
        assert!(url.contains("C:/data/test.db"));
        assert!(!url.contains('\\'));
        assert!(url.ends_with("?mode=rwc"));
    }
}
