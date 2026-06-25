//! Typed errors for LLM provider operations.
//!
//! Every fallible call into a provider returns `Result<T, LlmError>` per
//! `rules.md` §5.3. Variants carry the originating `provider` name so logs
//! and IPC payloads can route on it without parsing message text. Bridges
//! into `AppError` via `#[from]` so the rest of the crate keeps using
//! `AppResult<T>` at boundaries.

use thiserror::Error;

/// Errors surfaced by every concrete `LlmProvider` / `EmbeddingProvider`
/// implementation.
#[derive(Debug, Error)]
pub enum LlmError {
    /// Network or socket-level failure reaching the provider endpoint.
    #[error("{provider}: connection failed: {message}")]
    ConnectionFailed {
        provider: &'static str,
        message: String,
    },

    /// Provider rejected credentials (HTTP 401/403 or equivalent).
    #[error("{provider}: authentication failed: {message}")]
    AuthFailed {
        provider: &'static str,
        message: String,
    },

    /// Provider rate limit hit (HTTP 429). `retry_after_seconds` is set
    /// when the provider returns a `Retry-After` header.
    #[error("{provider}: rate limited (retry after {retry_after_seconds:?}s)")]
    RateLimited {
        provider: &'static str,
        retry_after_seconds: Option<u64>,
    },

    /// Request exceeded the model's context window. Both values are in
    /// tokens. Producers must emit this error before sending the request,
    /// not after — the count is approximate but the comparison is local.
    #[error("{provider}: context exceeded: {requested_tokens} > {limit}")]
    ContextExceeded {
        provider: &'static str,
        requested_tokens: u32,
        limit: u32,
    },

    /// Provider returned a successful HTTP response whose body did not
    /// match the expected shape.
    #[error("{provider}: invalid response: {message}")]
    InvalidResponse {
        provider: &'static str,
        message: String,
    },

    /// Tool-call output failed JSON-Schema validation. `payload_preview`
    /// is truncated to 256 chars to keep error logs bounded.
    #[error("{provider}: schema validation failed: {payload_preview}")]
    SchemaValidationFailed {
        provider: &'static str,
        payload_preview: String,
    },

    /// SSE / streaming connection terminated before the `Done` chunk.
    #[error("{provider}: stream interrupted: {message}")]
    StreamInterrupted {
        provider: &'static str,
        message: String,
    },

    /// Provider is up but reported a transient internal failure
    /// (HTTP 5xx outside 503 backoff territory).
    #[error("{provider}: provider unavailable: {message}")]
    ProviderUnavailable {
        provider: &'static str,
        message: String,
    },

    /// Caller asked for a feature the concrete provider does not support
    /// (e.g. tool-use against a tools-disabled Ollama model).
    #[error("{provider}: unsupported feature: {feature}")]
    Unsupported {
        provider: &'static str,
        feature: &'static str,
    },
}

/// Render `err` together with its full `source()` chain.
///
/// reqwest collapses a streaming-body failure into the unhelpful
/// top-level `"error decoding response body"`; the actionable root
/// cause (`connection closed before message completed`, `operation
/// timed out`, a TLS/IO read error, …) lives one or more links down the
/// `source()` chain. `to_string()` alone drops it, which is why
/// `StreamInterrupted` errors were previously opaque. Walking the chain
/// keeps the real reason in the message so the failure is diagnosable.
#[must_use]
pub fn describe_error_chain(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut current = err.source();
    while let Some(cause) = current {
        let rendered = cause.to_string();
        // Skip a link only when the message already *ends* with its text
        // — the case where an outer error's Display already appended its
        // source (`write!("{outer}: {source}")`), so re-appending would
        // read "x: x". Using `ends_with` rather than `contains` avoids
        // dropping a short but distinct cause (e.g. "read") that merely
        // appears as a substring of an earlier link.
        if !rendered.is_empty() && !message.ends_with(&rendered) {
            message.push_str(": ");
            message.push_str(&rendered);
        }
        current = cause.source();
    }
    message
}

impl LlmError {
    /// Convenience constructor: convert a `reqwest::Error` into the
    /// closest matching variant. Connection / DNS / TLS failures map to
    /// `ConnectionFailed`; everything else to `ProviderUnavailable`.
    #[must_use]
    pub fn from_reqwest(provider: &'static str, err: &reqwest::Error) -> Self {
        if err.is_connect() || err.is_timeout() {
            Self::ConnectionFailed {
                provider,
                message: err.to_string(),
            }
        } else {
            Self::ProviderUnavailable {
                provider,
                message: err.to_string(),
            }
        }
    }

    /// Stable IPC code used by the frontend to branch UI messaging
    /// without parsing display strings.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ConnectionFailed { .. } => "LLM_CONNECTION_FAILED",
            Self::AuthFailed { .. } => "LLM_AUTH_FAILED",
            Self::RateLimited { .. } => "LLM_RATE_LIMITED",
            Self::ContextExceeded { .. } => "LLM_CONTEXT_EXCEEDED",
            Self::InvalidResponse { .. } => "LLM_INVALID_RESPONSE",
            Self::SchemaValidationFailed { .. } => "LLM_SCHEMA_VALIDATION_FAILED",
            Self::StreamInterrupted { .. } => "LLM_STREAM_INTERRUPTED",
            Self::ProviderUnavailable { .. } => "LLM_PROVIDER_UNAVAILABLE",
            Self::Unsupported { .. } => "LLM_UNSUPPORTED",
        }
    }

    /// Provider name that produced this error.
    #[must_use]
    pub fn provider(&self) -> &'static str {
        match self {
            Self::ConnectionFailed { provider, .. }
            | Self::AuthFailed { provider, .. }
            | Self::RateLimited { provider, .. }
            | Self::ContextExceeded { provider, .. }
            | Self::InvalidResponse { provider, .. }
            | Self::SchemaValidationFailed { provider, .. }
            | Self::StreamInterrupted { provider, .. }
            | Self::ProviderUnavailable { provider, .. }
            | Self::Unsupported { provider, .. } => provider,
        }
    }

    /// Short, actionable recovery guidance for the user — one line per
    /// variant. Unlike [`Self::code`] (a stable machine identifier the
    /// frontend branches on) this is human-facing copy the UI can surface
    /// next to the message. Contains no secrets and no raw provider
    /// internals (`rules.md` §9).
    #[must_use]
    pub fn recovery_hint(&self) -> &'static str {
        match self {
            Self::ConnectionFailed { .. } => {
                "Can't reach the provider — check the base URL in Settings → Providers and that the service is running."
            }
            Self::AuthFailed { .. } => "Check your API key in Settings → Providers.",
            Self::RateLimited { .. } => "Rate limited — wait a moment and retry.",
            Self::ContextExceeded { .. } => {
                "Input exceeds the model's context window — shorten it or pick a model with a larger context."
            }
            Self::InvalidResponse { .. } => {
                "The provider returned an unexpected response — retry, or switch providers if it persists."
            }
            Self::SchemaValidationFailed { .. } => {
                "The model's tool output failed schema validation — retry, or use a model with stronger tool-use support."
            }
            Self::StreamInterrupted { .. } => {
                "The stream dropped — retry; if it persists, check your network or the provider's status."
            }
            Self::ProviderUnavailable { .. } => {
                "The provider reported a temporary failure — wait a moment and retry."
            }
            Self::Unsupported { .. } => {
                "This model doesn't support that feature — choose a different model or provider."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_stable_per_variant() {
        let cases = [
            (
                LlmError::ConnectionFailed {
                    provider: "ollama",
                    message: "x".into(),
                },
                "LLM_CONNECTION_FAILED",
            ),
            (
                LlmError::AuthFailed {
                    provider: "openai",
                    message: "x".into(),
                },
                "LLM_AUTH_FAILED",
            ),
            (
                LlmError::RateLimited {
                    provider: "anthropic",
                    retry_after_seconds: Some(30),
                },
                "LLM_RATE_LIMITED",
            ),
            (
                LlmError::ContextExceeded {
                    provider: "openai",
                    requested_tokens: 250_000,
                    limit: 128_000,
                },
                "LLM_CONTEXT_EXCEEDED",
            ),
            (
                LlmError::Unsupported {
                    provider: "ollama",
                    feature: "tool_use",
                },
                "LLM_UNSUPPORTED",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code(), expected);
        }
    }

    #[test]
    fn recovery_hint_is_nonempty_per_variant() {
        let variants = [
            LlmError::ConnectionFailed {
                provider: "ollama",
                message: "x".into(),
            },
            LlmError::AuthFailed {
                provider: "openai",
                message: "x".into(),
            },
            LlmError::RateLimited {
                provider: "anthropic",
                retry_after_seconds: Some(30),
            },
            LlmError::ContextExceeded {
                provider: "openai",
                requested_tokens: 250_000,
                limit: 128_000,
            },
            LlmError::InvalidResponse {
                provider: "ollama",
                message: "x".into(),
            },
            LlmError::SchemaValidationFailed {
                provider: "openai",
                payload_preview: "x".into(),
            },
            LlmError::StreamInterrupted {
                provider: "anthropic",
                message: "x".into(),
            },
            LlmError::ProviderUnavailable {
                provider: "ollama",
                message: "x".into(),
            },
            LlmError::Unsupported {
                provider: "ollama",
                feature: "tool_use",
            },
        ];
        for err in variants {
            assert!(
                !err.recovery_hint().is_empty(),
                "empty recovery hint for {}",
                err.code()
            );
        }
    }

    #[test]
    fn provider_extracts_correctly() {
        let err = LlmError::AuthFailed {
            provider: "openai",
            message: "bad key".into(),
        };
        assert_eq!(err.provider(), "openai");
    }

    #[test]
    fn display_includes_provider_and_message() {
        let err = LlmError::InvalidResponse {
            provider: "ollama",
            message: "missing choices".into(),
        };
        let display = err.to_string();
        assert!(display.contains("ollama"));
        assert!(display.contains("missing choices"));
    }

    #[test]
    fn rate_limited_display_shows_retry_after() {
        let err = LlmError::RateLimited {
            provider: "openai",
            retry_after_seconds: Some(45),
        };
        assert!(err.to_string().contains("45"));
    }

    #[derive(Debug)]
    struct StubError {
        message: &'static str,
        source: Option<Box<StubError>>,
    }

    impl std::fmt::Display for StubError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.message)
        }
    }

    impl std::error::Error for StubError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_deref()
                .map(|s| s as &(dyn std::error::Error + 'static))
        }
    }

    #[test]
    fn describe_error_chain_appends_nested_sources() {
        // Mirrors reqwest's shape: a generic top-level body-decode
        // message wrapping the real transport cause.
        let err = StubError {
            message: "error decoding response body",
            source: Some(Box::new(StubError {
                message: "connection closed before message completed",
                source: None,
            })),
        };
        let rendered = describe_error_chain(&err);
        assert_eq!(
            rendered,
            "error decoding response body: connection closed before message completed"
        );
    }

    #[test]
    fn describe_error_chain_skips_duplicate_link_text() {
        let err = StubError {
            message: "operation timed out",
            source: Some(Box::new(StubError {
                message: "operation timed out",
                source: None,
            })),
        };
        // The duplicated link must not produce "x: x".
        assert_eq!(describe_error_chain(&err), "operation timed out");
    }

    #[test]
    fn describe_error_chain_keeps_substring_cause_that_is_not_a_suffix() {
        // "read" is a substring of the top-level message but a distinct
        // link in the chain — it must be preserved, not deduped away.
        let err = StubError {
            message: "error decoding response body",
            source: Some(Box::new(StubError {
                message: "read",
                source: Some(Box::new(StubError {
                    message: "connection reset",
                    source: None,
                })),
            })),
        };
        assert_eq!(
            describe_error_chain(&err),
            "error decoding response body: read: connection reset"
        );
    }
}
