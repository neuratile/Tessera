//! JSON Web Token issuance and verification (`jsonwebtoken`, `HS256`).

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Claim value stored in refresh tokens (`kind` payload field).
const REFRESH_KIND: &str = "refresh";

/// Claims shared by access and refresh tokens. Access tokens omit [`Claims::kind`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — user id (`UUID` string).
    pub sub: String,
    /// Normalized email.
    pub email: String,
    /// Issued-at (`UNIX` time, seconds).
    pub iat: i64,
    /// Expiry (`UNIX` time, seconds).
    pub exp: i64,
    /// Optional unique token id for revocation / diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    /// `Some("refresh")` for refresh tokens; omitted for access tokens.
    #[serde(rename = "kind", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Encode an access JWT.
///
/// # Errors
///
/// Returns [`AppError::Internal`] when encoding fails.
pub fn encode_access_token(
    user_id: &Uuid,
    email: &str,
    ttl_secs: u64,
    secret: &[u8],
) -> AppResult<String> {
    let now = chrono::Utc::now().timestamp();
    let exp = now + i64::try_from(ttl_secs).map_err(|_| {
        AppError::Config("jwt_access_ttl_secs does not fit in i64 seconds".into())
    })?;
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        iat: now,
        exp,
        jti: Some(Uuid::new_v4().to_string()),
        kind: None,
    };
    encode_jwt(&claims, secret)
}

/// Encode a refresh JWT (longer TTL, `kind: refresh`).
///
/// # Errors
///
/// Returns [`AppError::Internal`] when encoding fails.
pub fn encode_refresh_token(
    user_id: &Uuid,
    email: &str,
    ttl_secs: u64,
    secret: &[u8],
) -> AppResult<String> {
    let now = chrono::Utc::now().timestamp();
    let exp = now + i64::try_from(ttl_secs).map_err(|_| {
        AppError::Config("jwt_refresh_ttl_secs does not fit in i64 seconds".into())
    })?;
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        iat: now,
        exp,
        jti: Some(Uuid::new_v4().to_string()),
        kind: Some(REFRESH_KIND.into()),
    };
    encode_jwt(&claims, secret)
}

fn encode_jwt(claims: &Claims, secret: &[u8]) -> AppResult<String> {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("jwt encode failed: {e}")))
}

fn validation(leeway_secs: u64) -> Validation {
    let mut v = Validation::new(Algorithm::HS256);
    v.leeway = leeway_secs;
    v
}

/// Decode and validate an access JWT (must not be a refresh token).
///
/// # Errors
///
/// Returns [`AppError::Unauthorized`] when the token is invalid, expired, or
/// carries `kind: refresh`.
pub fn decode_access_token(token: &str, secret: &[u8], leeway_secs: u64) -> AppResult<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret),
        &validation(leeway_secs),
    )
    .map_err(|_| AppError::Unauthorized("invalid or expired access token".into()))?;

    if data.claims.kind.as_deref() == Some(REFRESH_KIND) {
        return Err(AppError::Unauthorized("expected access token".into()));
    }

    Uuid::parse_str(&data.claims.sub)
        .map_err(|_| AppError::Unauthorized("invalid token subject".into()))?;

    Ok(data.claims)
}

/// Decode and validate a refresh JWT (`kind: refresh`).
///
/// # Errors
///
/// Returns [`AppError::Unauthorized`] when validation fails.
pub fn decode_refresh_token(token: &str, secret: &[u8], leeway_secs: u64) -> AppResult<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret),
        &validation(leeway_secs),
    )
    .map_err(|_| AppError::Unauthorized("invalid or expired refresh token".into()))?;

    if data.claims.kind.as_deref() != Some(REFRESH_KIND) {
        return Err(AppError::Unauthorized("expected refresh token".into()));
    }

    Uuid::parse_str(&data.claims.sub)
        .map_err(|_| AppError::Unauthorized("invalid token subject".into()))?;

    Ok(data.claims)
}
