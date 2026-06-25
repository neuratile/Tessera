//! Tracker integration errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Transport error: {0}")]
    Transport(String),
}

impl TrackerError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::AuthFailed(_) => "TRACKER_AUTH_FAILED",
            Self::RateLimited(_) => "TRACKER_RATE_LIMITED",
            Self::NotFound(_) => "TRACKER_NOT_FOUND",
            Self::InvalidRequest(_) => "TRACKER_INVALID_REQUEST",
            Self::Transport(_) => "TRACKER_TRANSPORT",
        }
    }

    #[must_use]
    pub fn from_http_status(status: reqwest::StatusCode, message: &str) -> Self {
        match status.as_u16() {
            401 | 403 => Self::AuthFailed(message.to_string()),
            404 => Self::NotFound(message.to_string()),
            429 => Self::RateLimited(message.to_string()),
            400 => Self::InvalidRequest(message.to_string()),
            _ => Self::Transport(format!("HTTP {status}: {message}")),
        }
    }

    /// Short, actionable recovery guidance for the user — one line per
    /// variant. Mirrors [`Self::code`] in shape but is human-facing copy the
    /// UI can surface next to the message; contains no secrets or raw
    /// provider internals (`rules.md` §9).
    #[must_use]
    pub fn recovery_hint(&self) -> &'static str {
        match self {
            Self::AuthFailed(_) => "Check the tracker's API token in Settings → Providers.",
            Self::RateLimited(_) => "Rate limited by the tracker — wait a moment and retry.",
            Self::NotFound(_) => {
                "The item doesn't exist or isn't visible to your token — verify the ID and the token's access."
            }
            Self::InvalidRequest(_) => {
                "The tracker rejected the request as invalid — check the inputs and try again."
            }
            Self::Transport(_) => {
                "Couldn't reach the tracker — check your network and the tracker's status."
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
            (TrackerError::AuthFailed("x".into()), "TRACKER_AUTH_FAILED"),
            (TrackerError::RateLimited("x".into()), "TRACKER_RATE_LIMITED"),
            (TrackerError::NotFound("x".into()), "TRACKER_NOT_FOUND"),
            (
                TrackerError::InvalidRequest("x".into()),
                "TRACKER_INVALID_REQUEST",
            ),
            (TrackerError::Transport("x".into()), "TRACKER_TRANSPORT"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code(), expected);
        }
    }

    #[test]
    fn recovery_hint_is_nonempty_per_variant() {
        let variants = [
            TrackerError::AuthFailed("x".into()),
            TrackerError::RateLimited("x".into()),
            TrackerError::NotFound("x".into()),
            TrackerError::InvalidRequest("x".into()),
            TrackerError::Transport("x".into()),
        ];
        for err in variants {
            assert!(
                !err.recovery_hint().is_empty(),
                "empty recovery hint for {}",
                err.code()
            );
        }
    }
}
