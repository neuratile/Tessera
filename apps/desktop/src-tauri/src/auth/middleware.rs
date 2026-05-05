//! JWT verification helpers for Tauri commands (“middleware” adapted for
//! IPC — there is no HTTP stack on the desktop backend).

use crate::auth::jwt::{decode_access_token, Claims};
use crate::error::{AppError, AppResult};

/// Strips a `Bearer ` prefix (case-insensitive) and returns the token slice.
///
/// # Errors
///
/// Returns [`AppError::InvalidInput`] when the header is missing a token.
pub fn strip_bearer(authorization: &str) -> AppResult<&str> {
    let s = authorization.trim();
    let rest = s
        .strip_prefix("Bearer ")
        .or_else(|| s.strip_prefix("bearer "))
        .ok_or_else(|| AppError::InvalidInput("expected Bearer token".into()))?;
    let token = rest.trim();
    if token.is_empty() {
        return Err(AppError::InvalidInput("missing bearer token".into()));
    }
    Ok(token)
}

/// Verifies `authorization` (raw `Authorization` header style value) as an
/// access JWT and returns decoded claims.
///
/// # Errors
///
/// Propagates [`AppError::InvalidInput`] / [`AppError::Unauthorized`] from
/// [`strip_bearer`] or [`decode_access_token`].
pub fn verify_bearer_access(
    authorization: &str,
    secret: &[u8],
    leeway_secs: u64,
) -> AppResult<Claims> {
    let token = strip_bearer(authorization)?;
    decode_access_token(token, secret, leeway_secs)
}
