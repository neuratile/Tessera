//! Application error type.
//!
//! Per `rules.md` §2.2 + §5.3: every fallible operation returns
//! `Result<T, AppError>`. `thiserror` powers the variants so each carries a
//! typed cause and a stable identifier for the IPC boundary.

use std::io;

use thiserror::Error;

use crate::providers::llm::LlmError;

/// Top-level error type for all backend operations.
///
/// Variants stay coarse on purpose — they map onto IPC-facing error codes,
/// not internal call-site details. Add a new variant only when callers need
/// to handle the case differently from the existing ones.
#[derive(Debug, Error)]
pub enum AppError {
    /// Configuration could not be loaded or was invalid.
    #[error("configuration error: {0}")]
    Config(String),

    /// Database operation failed.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Database migration failed.
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// Filesystem or other I/O failure.
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// HTTP request to an external provider failed.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// LLM provider returned an error or invalid response. Bridges from
    /// `providers::llm::LlmError` (rules.md §5.3 — typed errors propagate).
    #[error("llm error: {0}")]
    Llm(#[from] LlmError),

    /// Requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Authentication failed or the caller lacked a valid credential.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// Caller-supplied input failed validation at a trust boundary
    /// (`rules.md` §9).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Operation rejected because it would exceed a configured safety
    /// limit (file size, project size, context window, etc.).
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),

    /// Catch-all for unexpected failures wrapping `anyhow::Error`. Prefer
    /// a specific variant; only use this when a third-party crate returns
    /// an opaque error that we do not need to inspect.
    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// Stable string code for IPC consumers (frontend) and logs. Never
    /// surfaces internal detail; safe for user-facing display per
    /// `rules.md` §5.3.
    ///
    /// `Llm` variants delegate to the inner `LlmError::code()` so the
    /// frontend sees the same fine-grained codes (e.g.
    /// `LLM_RATE_LIMITED`) regardless of how deep the error originated.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Config(_) => "CONFIG_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Migration(_) => "MIGRATION_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Http(_) => "HTTP_ERROR",
            Self::Serde(_) => "SERIALIZATION_ERROR",
            Self::Llm(inner) => inner.code(),
            Self::NotFound(_) => "NOT_FOUND",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::LimitExceeded(_) => "LIMIT_EXCEEDED",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

/// Convenience alias used throughout the crate.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_stable_for_each_variant() {
        let cases = [
            (AppError::Config("x".into()), "CONFIG_ERROR"),
            (AppError::NotFound("x".into()), "NOT_FOUND"),
            (AppError::Unauthorized("x".into()), "UNAUTHORIZED"),
            (AppError::InvalidInput("x".into()), "INVALID_INPUT"),
            (AppError::LimitExceeded("x".into()), "LIMIT_EXCEEDED"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code(), expected);
        }
    }

    #[test]
    fn llm_variant_delegates_code_to_inner_error() {
        let inner = LlmError::AuthFailed {
            provider: "openai",
            message: "bad key".into(),
        };
        let app_err: AppError = inner.into();
        assert_eq!(app_err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn display_includes_inner_message() {
        let err = AppError::InvalidInput("bad path".into());
        assert!(err.to_string().contains("bad path"));
    }

    #[test]
    fn io_error_converts_via_from() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "missing");
        let app_err: AppError = io_err.into();
        assert_eq!(app_err.code(), "IO_ERROR");
    }
}
