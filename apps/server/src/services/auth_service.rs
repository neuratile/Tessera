//! Authentication business logic.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::middleware::auth::Claims;
use crate::models::user::{CreateUser, User, UserProfile};

/// JWT tokens returned upon login or token refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserProfile,
}

/// Hash a password using Argon2.
pub fn hash_password(password: &str) -> ApiResult<String> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(format!("password hashing failed: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a password against a hash using Argon2.
pub fn verify_password(password: &str, password_hash: &str) -> ApiResult<()> {
    if password_hash.is_empty() {
        return Err(ApiError::Auth("invalid email or password".into()));
    }
    let parsed = PasswordHash::new(password_hash)
        .map_err(|_| ApiError::Auth("invalid email or password".into()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| ApiError::Auth("invalid email or password".into()))
}

/// Generate access and refresh tokens for a user.
pub fn generate_tokens(user_id: Uuid, config: &Config) -> ApiResult<(String, String)> {
    let now = Utc::now().timestamp();
    
    // Access token: 1 hour expiry
    let access_exp = Utc::now()
        .checked_add_signed(Duration::hours(1))
        .ok_or_else(|| ApiError::Internal("failed to calculate token expiry".into()))?
        .timestamp();
        
    let access_claims = Claims {
        sub: user_id.to_string(),
        iat: now,
        exp: access_exp,
        kind: None,
    };
    
    let access_token = encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("failed to generate access token: {e}")))?;

    // Refresh token: 7 days expiry
    let refresh_exp = Utc::now()
        .checked_add_signed(Duration::days(7))
        .ok_or_else(|| ApiError::Internal("failed to calculate token expiry".into()))?
        .timestamp();
        
    let refresh_claims = Claims {
        sub: user_id.to_string(),
        iat: now,
        exp: refresh_exp,
        kind: Some("refresh".to_string()),
    };
    
    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("failed to generate refresh token: {e}")))?;

    Ok((access_token, refresh_token))
}

/// Register a new user.
pub async fn register(pool: &PgPool, config: &Config, payload: CreateUser) -> ApiResult<TokenResponse> {
    let email = payload.email.trim().to_lowercase();
    if email.is_empty() || payload.display_name.trim().is_empty() || payload.password.len() < 6 {
        return Err(ApiError::Validation("invalid email, display name, or password too short".into()));
    }

    // Check if user already exists
    let existing = sqlx::query("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(pool)
        .await?;

    if existing.is_some() {
        return Err(ApiError::Validation("email already in use".into()));
    }

    let password_hash = hash_password(&payload.password)?;
    let user_id = Uuid::new_v4();
    let now = Utc::now();

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (id, email, display_name, password_hash, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, email, display_name, avatar_url, password_hash, created_at, updated_at
        "#,
    )
    .bind(user_id)
    .bind(email)
    .bind(payload.display_name.trim())
    .bind(password_hash)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    let (access_token, refresh_token) = generate_tokens(user.id, config)?;

    let profile = UserProfile {
        id: user.id,
        email: user.email,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        created_at: user.created_at,
        updated_at: user.updated_at,
    };

    Ok(TokenResponse {
        access_token,
        refresh_token,
        user: profile,
    })
}

/// Authenticate a user by email and password.
pub async fn login(pool: &PgPool, config: &Config, email: &str, password: &str) -> ApiResult<TokenResponse> {
    let email = email.trim().to_lowercase();
    
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, display_name, avatar_url, password_hash, created_at, updated_at FROM users WHERE email = $1"
    )
    .bind(email)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::Auth("invalid email or password".into()))?;

    verify_password(password, user.password_hash.as_deref().unwrap_or(""))?;

    let (access_token, refresh_token) = generate_tokens(user.id, config)?;

    let profile = UserProfile {
        id: user.id,
        email: user.email,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        created_at: user.created_at,
        updated_at: user.updated_at,
    };

    Ok(TokenResponse {
        access_token,
        refresh_token,
        user: profile,
    })
}

/// Retrieve a user profile by ID.
pub async fn get_user_profile(pool: &PgPool, user_id: Uuid) -> ApiResult<UserProfile> {
    let profile = sqlx::query_as::<_, UserProfile>(
        "SELECT id, email, display_name, avatar_url, created_at, updated_at FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

    Ok(profile)
}

