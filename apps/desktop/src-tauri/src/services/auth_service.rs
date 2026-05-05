//! Auth orchestration: registration, login, refresh (`commands` stay thin).

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::auth::{
    self, canonical_email, encode_access_token, encode_refresh_token, validate_login,
    validate_register, Claims, LoginDto, RegisterDto,
};
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::repositories::user_repo;

/// OAuth2-style token pair returned to the desktop shell.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
}

/// Public session snapshot for protected commands.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
}

/// Registers a user, persists credentials, and returns fresh tokens.
///
/// # Errors
///
/// Propagates validation, hashing, and database errors.
pub async fn register(pool: &SqlitePool, cfg: &AppConfig, dto: &RegisterDto) -> AppResult<TokenPair> {
    validate_register(dto)?;
    let email = canonical_email(&dto.email)?;
    if user_repo::email_exists(pool, &email).await? {
        return Err(AppError::InvalidInput("email already registered".into()));
    }

    let password_hash = auth::hash_password(&dto.password)?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let name_trimmed = dto.name.as_ref().map(|n| n.trim().to_string());
    let name_ref = name_trimmed.as_deref();

    user_repo::insert_user(pool, &id, &email, name_ref, &password_hash, &now, &now).await?;

    issue_pair(cfg, &id, &email)
}

/// Validates credentials and returns a token pair.
///
/// # Errors
///
/// Returns [`AppError::Unauthorized`] for bad credentials.
pub async fn login(pool: &SqlitePool, cfg: &AppConfig, dto: &LoginDto) -> AppResult<TokenPair> {
    validate_login(dto)?;
    let email = canonical_email(&dto.email)?;
    let row = user_repo::find_auth_by_email(pool, &email).await?;
    auth::verify_password(&dto.password, &row.password_hash)?;
    issue_pair(cfg, &row.id, &row.email)
}

/// Rotates tokens from a valid refresh JWT after re-checking the user row.
///
/// # Errors
///
/// Returns [`AppError::Unauthorized`] when the refresh token is invalid or the
/// user no longer exists.
pub async fn refresh_tokens(
    pool: &SqlitePool,
    cfg: &AppConfig,
    refresh_token: &str,
) -> AppResult<TokenPair> {
    let claims = auth::decode_refresh_token(refresh_token, cfg.jwt_secret.as_bytes(), 60)?;
    let user = user_repo::find_user_by_id(pool, &claims.sub).await?;
    if user.email != claims.email {
        return Err(AppError::Unauthorized("invalid refresh token".into()));
    }
    issue_pair(cfg, &user.id, &user.email)
}

/// Returns the caller's profile after JWT verification (`middleware` hook).
///
/// # Errors
///
/// Propagates [`AppError::Unauthorized`] / [`AppError::NotFound`].
pub async fn session_from_access_claims(pool: &SqlitePool, claims: &Claims) -> AppResult<SessionUser> {
    let user = user_repo::find_user_by_id(pool, &claims.sub).await?;
    if user.email != claims.email {
        return Err(AppError::Unauthorized("invalid access token".into()));
    }
    Ok(SessionUser {
        id: user.id,
        email: user.email,
        name: user.name,
    })
}

fn issue_pair(cfg: &AppConfig, user_id: &str, email: &str) -> AppResult<TokenPair> {
    let uuid = Uuid::parse_str(user_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("stored user id is not a uuid")))?;
    let access = encode_access_token(
        &uuid,
        email,
        cfg.jwt_access_ttl_secs,
        cfg.jwt_secret.as_bytes(),
    )?;
    let refresh = encode_refresh_token(
        &uuid,
        email,
        cfg.jwt_refresh_ttl_secs,
        cfg.jwt_secret.as_bytes(),
    )?;
    Ok(TokenPair {
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    use crate::db;

    fn test_cfg() -> AppConfig {
        AppConfig {
            ollama_base_url: crate::config::DEFAULT_OLLAMA_BASE_URL.into(),
            db_path: None,
            log_level: crate::config::DEFAULT_LOG_LEVEL.into(),
            jwt_secret: crate::config::DEFAULT_JWT_SECRET_DEV.into(),
            jwt_access_ttl_secs: 300,
            jwt_refresh_ttl_secs: 3_600,
            sentry_dsn: None,
        }
    }

    #[tokio::test]
    async fn register_login_refresh_round_trip() {
        let tmp = env::temp_dir().join(format!("testing-ide-auth-{}.db", Uuid::new_v4()));
        let pool = db::init_pool_at(&tmp).await.expect("pool");

        let cfg = test_cfg();
        let reg = RegisterDto {
            email: "Tester@Example.com".into(),
            password: "password1".into(),
            name: Some("Tester".into()),
        };
        let pair = register(&pool, &cfg, &reg).await.expect("register");
        assert!(!pair.access_token.is_empty());

        let logged_in = login(
            &pool,
            &cfg,
            &LoginDto {
                email: "tester@example.com".into(),
                password: "password1".into(),
            },
        )
        .await
        .expect("login");

        assert_ne!(logged_in.access_token, pair.access_token);

        let refreshed = refresh_tokens(&pool, &cfg, &logged_in.refresh_token)
            .await
            .expect("refresh");
        assert!(!refreshed.access_token.is_empty());

        let claims = auth::decode_access_token(
            &refreshed.access_token,
            cfg.jwt_secret.as_bytes(),
            60,
        )
        .expect("decode access");
        let session = session_from_access_claims(&pool, &claims)
            .await
            .expect("session");
        assert_eq!(session.email, "tester@example.com");

        pool.close().await;
        let _ = std::fs::remove_file(&tmp);
    }
}
