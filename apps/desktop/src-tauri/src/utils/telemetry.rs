//! Structured logging setup.
//!
//! Per `rules.md` §5.4: tracing only — no `println!`, no `eprintln!`. JSON
//! output in release builds for ingestion (Sentry / log aggregator);
//! pretty output in debug builds for developer ergonomics.

use std::sync::OnceLock;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::error::{AppError, AppResult};

static INIT: OnceLock<()> = OnceLock::new();

/// Initialize the global `tracing` subscriber. Idempotent — safe to call
/// from `main` and from integration tests; subsequent calls are no-ops.
///
/// `filter_directive` follows the `EnvFilter` syntax (e.g. `info` or
/// `testing_ide_lib=debug,sqlx=warn`).
///
/// # Errors
///
/// Returns `AppError::Config` if `filter_directive` is not a valid
/// `EnvFilter` expression.
pub fn init(filter_directive: &str) -> AppResult<()> {
    // Validate the filter eagerly so misconfiguration surfaces as a
    // typed error on every call, not silently no-op'd after the first
    // successful init. Otherwise a later call with a bad LOG_LEVEL
    // (e.g. from a config reload) would return Ok(()).
    let filter = EnvFilter::try_new(filter_directive)
        .map_err(|e| AppError::Config(format!("invalid LOG_LEVEL '{filter_directive}': {e}")))?;

    if INIT.get().is_some() {
        return Ok(());
    }

    let layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false);

    let registry = tracing_subscriber::registry().with(filter);

    #[cfg(debug_assertions)]
    let result = registry.with(layer.pretty()).try_init();

    #[cfg(not(debug_assertions))]
    let result = registry.with(layer.json()).try_init();

    if result.is_ok() {
        let _ = INIT.set(());
    }
    Ok(())
}

/// Initializes native Sentry reporting when a DSN is configured.
///
/// Returns a guard that must stay alive for the duration of the process. The
/// Tauri entrypoint keeps it in a stack binding until shutdown.
#[must_use]
pub(crate) fn init_sentry(dsn: Option<&str>) -> Option<sentry::ClientInitGuard> {
    let dsn = normalize_sentry_dsn(dsn)?;

    Some(sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            environment: Some(default_sentry_environment().into()),
            attach_stacktrace: true,
            debug: cfg!(debug_assertions),
            ..Default::default()
        },
    )))
}

fn normalize_sentry_dsn(dsn: Option<&str>) -> Option<&str> {
    dsn.map(str::trim).filter(|value| !value.is_empty())
}

fn default_sentry_environment() -> &'static str {
    if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_filter_directive_is_rejected() {
        let err = init("=== invalid ===").expect_err("invalid filter must error");
        assert_eq!(err.code(), "CONFIG_ERROR");
    }

    #[test]
    fn init_is_idempotent() {
        init("info").expect("first init must succeed");
        init("info").expect("second init must be a no-op");
    }

    #[test]
    fn normalize_sentry_dsn_discards_missing_or_blank_values() {
        assert_eq!(normalize_sentry_dsn(None), None);
        assert_eq!(normalize_sentry_dsn(Some("   ")), None);
        assert_eq!(
            normalize_sentry_dsn(Some(" https://example.test/123 ")),
            Some("https://example.test/123")
        );
    }

    #[test]
    fn default_sentry_environment_matches_build_mode() {
        #[cfg(debug_assertions)]
        assert_eq!(default_sentry_environment(), "development");

        #[cfg(not(debug_assertions))]
        assert_eq!(default_sentry_environment(), "production");
    }
}
