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
    let exp = now
        + i64::try_from(ttl_secs).map_err(|_| {
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
    let exp = now
        + i64::try_from(ttl_secs).map_err(|_| {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_secret() -> &'static [u8] {
        b"0123456789abcdef0123456789abcdef"
    }

    fn test_user_id() -> Uuid {
        Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("uuid")
    }

    #[test]
    fn access_token_round_trips() {
        let token = encode_access_token(&test_user_id(), "user@example.com", 300, test_secret())
            .expect("encode");

        let claims = decode_access_token(&token, test_secret(), 0).expect("decode");
        assert_eq!(claims.sub, test_user_id().to_string());
        assert_eq!(claims.email, "user@example.com");
        assert_eq!(claims.kind, None);
        assert!(claims.jti.is_some());
    }

    #[test]
    fn refresh_token_round_trips() {
        let token = encode_refresh_token(&test_user_id(), "user@example.com", 3_600, test_secret())
            .expect("encode");

        let claims = decode_refresh_token(&token, test_secret(), 0).expect("decode");
        assert_eq!(claims.sub, test_user_id().to_string());
        assert_eq!(claims.email, "user@example.com");
        assert_eq!(claims.kind.as_deref(), Some(REFRESH_KIND));
        assert!(claims.jti.is_some());
    }

    #[test]
    fn access_decoder_rejects_refresh_tokens() {
        let refresh =
            encode_refresh_token(&test_user_id(), "user@example.com", 3_600, test_secret())
                .expect("encode");

        let err = decode_access_token(&refresh, test_secret(), 0).expect_err("must reject");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }

    #[test]
    fn refresh_decoder_rejects_access_tokens() {
        let access = encode_access_token(&test_user_id(), "user@example.com", 300, test_secret())
            .expect("encode");

        let err = decode_refresh_token(&access, test_secret(), 0).expect_err("must reject");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }

    #[test]
    fn decoders_reject_wrong_secret() {
        let access = encode_access_token(&test_user_id(), "user@example.com", 300, test_secret())
            .expect("encode");

        let err = decode_access_token(&access, b"abcdef0123456789abcdef0123456789", 0)
            .expect_err("must reject");
        assert_eq!(err.code(), "UNAUTHORIZED");
    }
}
