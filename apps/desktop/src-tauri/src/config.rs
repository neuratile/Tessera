//! Typed application configuration loaded from environment variables.
//!
//! Per `rules.md` §13: env access is centralized here, never scattered as
//! `std::env::var(...)` calls throughout the codebase. All variables have
//! defaults so the app boots in a fresh checkout without an `.env` file.
//!
//! Phase 6: optional [`load_dotenv_optional`] merges `apps/desktop/.env` /
//! `./.env` into the process env before [`AppConfig::from_env`].

use std::env;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Loads `.env` when present (`dotenvy`). Call from the binary `main` before
/// `testing_ide_lib::run`.
///
/// Resolution order:
/// 1. [`dotenvy::dotenv`] (current working directory — matches `pnpm tauri dev`
///    when cwd is `apps/desktop`).
/// 2. `<crate manifest>/../.env` (`apps/desktop/.env` next to `package.json`) when
///    step 1 finds no file (`cargo run` from `apps/desktop/src-tauri`).
///
/// Missing files are **not** an error (`rules.md` — defaults remain valid).
pub fn load_dotenv_optional() {
    if dotenvy::dotenv().is_ok() {
        return;
    }
    let desktop_env = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.env");
    let _ignored = dotenvy::from_path(desktop_env);
}

/// Default Ollama base URL (OpenAI-compatible endpoint).
pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Default `SQLite` filename, resolved relative to the user data directory.
pub const DEFAULT_DB_FILENAME: &str = "testing-ide.db";

/// Default tracing filter directive.
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// Development-only `JWT` signing secret. **Not for production** — override with
/// `JWT_SECRET` (minimum 32 bytes) before shipping a multi-user build.
pub const DEFAULT_JWT_SECRET_DEV: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Default access-token lifetime (15 minutes).
pub const DEFAULT_JWT_ACCESS_TTL_SECS: u64 = 900;

/// Default refresh-token lifetime (7 days).
pub const DEFAULT_JWT_REFRESH_TTL_SECS: u64 = 60 * 60 * 24 * 7;

/// Minimum accepted length for `JWT_SECRET` (HS256 key material).
pub const MIN_JWT_SECRET_BYTES: usize = 32;

/// Strongly-typed configuration assembled from the process environment.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Base URL for the Ollama OpenAI-compatible API (`OLLAMA_BASE_URL`).
    pub ollama_base_url: String,
    /// Explicit `SQLite` file path (`DB_PATH`) when overriding the default
    /// `<app_local_data_dir>/testing-ide.db` layout (`None` = use app data dir).
    pub db_path: Option<PathBuf>,
    /// `tracing_subscriber::EnvFilter` directive, e.g. `info` or
    /// `testing_ide_lib=debug,sqlx=warn`.
    pub log_level: String,
    /// HMAC secret for signing `JWT`s (`HS256`). Must be at least
    /// [`MIN_JWT_SECRET_BYTES`] octets when overridden via `JWT_SECRET`.
    pub jwt_secret: String,
    /// Access-token TTL in seconds.
    pub jwt_access_ttl_secs: u64,
    /// Refresh-token TTL in seconds.
    pub jwt_refresh_ttl_secs: u64,
    /// Optional `Sentry` DSN for native crash/error reporting (`SENTRY_DSN`).
    ///
    /// `None` when unset or whitespace-only — reporting hooks remain disabled until
    /// a later phase wires the SDK.
    pub sentry_dsn: Option<String>,
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
        let db_path = read_optional_explicit_db_path()?;
        let log_level = read_string("LOG_LEVEL", DEFAULT_LOG_LEVEL)?;
        let jwt_secret = read_jwt_secret()?;
        let jwt_access_ttl_secs = read_u64(
            "JWT_ACCESS_TTL_SECS",
            DEFAULT_JWT_ACCESS_TTL_SECS,
            60,
            24 * 60 * 60,
        )?;
        let jwt_refresh_ttl_secs = read_u64(
            "JWT_REFRESH_TTL_SECS",
            DEFAULT_JWT_REFRESH_TTL_SECS,
            300,
            365 * 24 * 60 * 60,
        )?;
        let sentry_dsn = read_optional_non_empty_trimmed("SENTRY_DSN")?;

        Ok(Self {
            ollama_base_url,
            db_path,
            log_level,
            jwt_secret,
            jwt_access_ttl_secs,
            jwt_refresh_ttl_secs,
            sentry_dsn,
        })
    }

    /// `sqlx`-compatible connection string for an explicit `SQLite` override.
    ///
    /// Returns [`None`] when [`Self::db_path`] is absent (caller should open the
    /// pool via [`crate::db::init_pool_at`] / Tauri-managed path instead).
    #[must_use]
    pub fn database_url_override(&self) -> Option<String> {
        self.db_path
            .as_ref()
            .map(|path| sqlite_file_connection_url(path))
    }
}

/// Build a sqlx-compatible file URL (`?mode=rwc`) using forward slashes.
#[must_use]
pub fn sqlite_file_connection_url(path: &Path) -> String {
    let path = path.display().to_string().replace('\\', "/");
    format!("sqlite://{path}?mode=rwc")
}

fn read_optional_explicit_db_path() -> AppResult<Option<PathBuf>> {
    match env::var("DB_PATH") {
        Ok(value) if value.trim().is_empty() => Err(AppError::Config(
            "environment variable DB_PATH is set but empty".into(),
        )),
        Ok(value) => crate::utils::sqlite_path::parse_sqlite_path_override(&value).map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(AppError::Config(
            "environment variable DB_PATH is not valid unicode".into(),
        )),
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

fn read_jwt_secret() -> AppResult<String> {
    match env::var("JWT_SECRET") {
        Ok(value) if value.trim().is_empty() => Err(AppError::Config(
            "environment variable JWT_SECRET is set but empty".into(),
        )),
        Ok(value) => {
            if value.len() < MIN_JWT_SECRET_BYTES {
                return Err(AppError::Config(format!(
                    "JWT_SECRET must be at least {MIN_JWT_SECRET_BYTES} bytes"
                )));
            }
            Ok(value)
        }
        Err(env::VarError::NotPresent) => Ok(DEFAULT_JWT_SECRET_DEV.to_string()),
        Err(env::VarError::NotUnicode(_)) => Err(AppError::Config(
            "environment variable JWT_SECRET is not valid unicode".into(),
        )),
    }
}

fn read_u64(key: &str, default: u64, min: u64, max: u64) -> AppResult<u64> {
    let raw = match env::var(key) {
        Ok(value) if value.trim().is_empty() => {
            return Err(AppError::Config(format!(
                "environment variable {key} is set but empty"
            )));
        }
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(default),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(AppError::Config(format!(
                "environment variable {key} is not valid unicode"
            )));
        }
    };
    let parsed = raw
        .parse::<u64>()
        .map_err(|_| AppError::Config(format!("environment variable {key} must be a u64")))?;
    if !(min..=max).contains(&parsed) {
        return Err(AppError::Config(format!(
            "environment variable {key} must be between {min} and {max} inclusive"
        )));
    }
    Ok(parsed)
}

fn read_optional_non_empty_trimmed(key: &str) -> AppResult<Option<String>> {
    match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.into()))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(AppError::Config(format!(
            "environment variable {key} is not valid unicode"
        ))),
    }
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
        env::remove_var("JWT_SECRET");
        env::remove_var("JWT_ACCESS_TTL_SECS");
        env::remove_var("JWT_REFRESH_TTL_SECS");
        env::remove_var("SENTRY_DSN");
    }

    #[test]
    fn defaults_apply_when_env_unset() {
        let _g = env_guard();
        clear_vars();

        let cfg = AppConfig::from_env().expect("defaults must always load");
        assert_eq!(cfg.ollama_base_url, DEFAULT_OLLAMA_BASE_URL);
        assert_eq!(cfg.log_level, DEFAULT_LOG_LEVEL);
        assert!(cfg.db_path.is_none(), "unset DB_PATH must defer to app data dir resolver");
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
    fn empty_db_path_override_is_rejected() {
        let _g = env_guard();
        clear_vars();
        env::set_var("DB_PATH", "   ");

        let err = AppConfig::from_env().expect_err("empty DB_PATH must error");
        assert_eq!(err.code(), "CONFIG_ERROR");

        clear_vars();
    }

    #[test]
    fn db_path_override_optional_round_trips_relative() {
        let _g = env_guard();
        clear_vars();

        env::set_var("DB_PATH", " ./phase6-relative.db ");

        let cfg = AppConfig::from_env().expect("db override");
        let url = cfg.database_url_override().expect("explicit path url");
        assert!(url.starts_with("sqlite://"));
        assert!(url.ends_with("?mode=rwc"));
        assert!(
            url.contains("phase6-relative.db"),
            "unexpected sqlite url {url:?}"
        );

        clear_vars();
    }

    #[test]
    fn jwt_secret_override_must_meet_minimum_length() {
        let _g = env_guard();
        clear_vars();
        env::set_var("JWT_SECRET", "too-short");

        let err = AppConfig::from_env().expect_err("short secret must error");
        assert_eq!(err.code(), "CONFIG_ERROR");

        clear_vars();
    }

    #[test]
    fn sentry_dsn_optional_empty_means_disabled() {
        let _g = env_guard();
        clear_vars();

        env::set_var("SENTRY_DSN", "   ");

        let cfg = AppConfig::from_env().expect("whitespace sentry must behave as absent");
        assert!(cfg.sentry_dsn.is_none());

        clear_vars();
    }

    #[test]
    fn sentry_dsn_persists_when_trimmed_non_empty() {
        let _g = env_guard();
        clear_vars();

        env::set_var("SENTRY_DSN", " https://example.test/abc ");

        let cfg = AppConfig::from_env().expect("valid sentry uri");
        assert_eq!(cfg.sentry_dsn.as_deref(), Some("https://example.test/abc"));

        clear_vars();
    }

    #[test]
    fn sqlite_file_connection_url_uses_forward_slashes() {
        let path = PathBuf::from(r"C:\data\test.db");
        let url = sqlite_file_connection_url(&path);
        assert!(url.starts_with("sqlite://"));
        assert!(url.contains("C:/data/test.db"));
        assert!(!url.contains('\\'));
        assert!(url.ends_with("?mode=rwc"));
    }
}
