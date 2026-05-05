//! Argon2 password hashing (`rules.md` §2.2 — no `unwrap` on user paths).

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use crate::error::{AppError, AppResult};

/// Hash `password` for storage in `users.password_hash` (`PHC` string format).
///
/// # Errors
///
/// Returns [`AppError::Internal`] when the OS RNG or hasher fails.
pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hash failed: {e}")))?;
    Ok(hash.to_string())
}

/// Constant-time verify of `password` against a `PHC` string.
///
/// # Errors
///
/// Returns [`AppError::Unauthorized`] when the hash is malformed or the
/// password does not match.
pub fn verify_password(password: &str, password_hash: &str) -> AppResult<()> {
    if password_hash.is_empty() {
        return Err(AppError::Unauthorized("invalid credentials".into()));
    }
    let parsed = PasswordHash::new(password_hash)
        .map_err(|_| AppError::Unauthorized("invalid credentials".into()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized("invalid credentials".into()))
}
