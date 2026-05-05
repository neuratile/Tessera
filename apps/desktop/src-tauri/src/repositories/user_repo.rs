//! `users` table access (`rules.md` §2.3 — parameterized queries only).

use sqlx::SqlitePool;

use crate::db::models::UserRow;
use crate::error::{AppError, AppResult};

/// Credential bundle loaded for `login`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserAuthRow {
    pub id: String,
    pub email: String,
    pub password_hash: String,
}

/// Returns `true` when `email` is already taken (case-insensitive canonical match
/// is enforced by storing lower-cased emails from [`crate::auth::canonical_email`]).
///
/// # Errors
///
/// Propagates [`AppError::Database`].
pub async fn email_exists(pool: &SqlitePool, email: &str) -> AppResult<bool> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(1) FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await?;
    Ok(row.0 > 0)
}

/// Inserts a new user row and returns its id.
///
/// # Errors
///
/// Propagates database errors (including unique violations as
/// [`AppError::Database`]).
pub async fn insert_user(
    pool: &SqlitePool,
    id: &str,
    email: &str,
    name: Option<&str>,
    password_hash: &str,
    created_at: &str,
    updated_at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO users (id, email, name, plan, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', ?, ?, ?)",
    )
    .bind(id)
    .bind(email)
    .bind(name)
    .bind(password_hash)
    .bind(created_at)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Loads auth fields for a user by email.
///
/// # Errors
///
/// Returns [`AppError::Unauthorized`] when no row exists (indistinguishable from a
/// bad password at the IPC layer).
pub async fn find_auth_by_email(pool: &SqlitePool, email: &str) -> AppResult<UserAuthRow> {
    sqlx::query_as::<_, UserAuthRow>("SELECT id, email, password_hash FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".into()))
}

/// Loads a full [`UserRow`] by primary key.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] when the id does not exist.
pub async fn find_user_by_id(pool: &SqlitePool, id: &str) -> AppResult<UserRow> {
    let row: Option<UserRow> = sqlx::query_as::<_, UserRow>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    row.ok_or_else(|| AppError::NotFound(format!("user {id} not found")))
}
