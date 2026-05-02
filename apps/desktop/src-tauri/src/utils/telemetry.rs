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
    if INIT.get().is_some() {
        return Ok(());
    }

    let filter = EnvFilter::try_new(filter_directive)
        .map_err(|e| AppError::Config(format!("invalid LOG_LEVEL '{filter_directive}': {e}")))?;

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
}
