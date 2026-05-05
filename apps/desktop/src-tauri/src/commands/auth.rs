//! Auth-related Tauri IPC handlers (`register`, `login`, `refresh_token`, `auth_me`).

use sqlx::SqlitePool;
use tauri::State;

use crate::auth::{verify_bearer_access, LoginDto, RegisterDto};
use crate::config::AppConfig;
use crate::services::auth_service::{self, SessionUser, TokenPair};

/// `refresh_token` IPC body (`camelCase` from the React shell).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshBody {
    /// Long-lived refresh JWT returned from [`register`](crate::commands::auth::register)
    /// or [`login`](crate::commands::auth::login).
    pub refresh_token: String,
}

/// Creates a user, stores an `Argon2` hash, and returns `JWT` access + refresh tokens.
///
/// # Errors
///
/// Returns a stringified [`crate::error::AppError`] for the frontend.
#[tauri::command]
pub async fn register(
    pool: State<'_, SqlitePool>,
    cfg: State<'_, AppConfig>,
    body: RegisterDto,
) -> Result<TokenPair, String> {
    auth_service::register(&pool, &cfg, &body)
        .await
        .map_err(|e| e.to_string())
}

/// Validates email/password and returns a fresh token pair.
///
/// # Errors
///
/// Returns `Err(String)` when validation fails or credentials are invalid.
#[tauri::command]
pub async fn login(
    pool: State<'_, SqlitePool>,
    cfg: State<'_, AppConfig>,
    body: LoginDto,
) -> Result<TokenPair, String> {
    auth_service::login(&pool, &cfg, &body)
        .await
        .map_err(|e| e.to_string())
}

/// Exchanges a valid refresh JWT for a new access + refresh pair.
///
/// # Errors
///
/// Returns `Err(String)` when the refresh token is invalid or the user row
/// disappeared.
#[tauri::command]
pub async fn refresh_token(
    pool: State<'_, SqlitePool>,
    cfg: State<'_, AppConfig>,
    body: RefreshBody,
) -> Result<TokenPair, String> {
    auth_service::refresh_tokens(&pool, &cfg, &body.refresh_token)
        .await
        .map_err(|e| e.to_string())
}

/// Protected command: verifies a `Bearer` access token then returns the user row.
///
/// Pass `authorization: "Bearer <access_token>"` from the frontend.
///
/// # Errors
///
/// Returns `Err(String)` when the header/token is malformed or the user is gone.
#[tauri::command]
pub async fn auth_me(
    pool: State<'_, SqlitePool>,
    cfg: State<'_, AppConfig>,
    authorization: String,
) -> Result<SessionUser, String> {
    let claims = verify_bearer_access(&authorization, cfg.jwt_secret.as_bytes(), 60)
        .map_err(|e| e.to_string())?;
    auth_service::session_from_access_claims(&pool, &claims)
        .await
        .map_err(|e| e.to_string())
}
