//! Request DTOs for auth IPC. Field validation mirrors `packages/shared`
//! `RegisterSchema` / `LoginSchema` (`rules.md` §2.1 — Zod on the frontend,
//! equivalent checks on the Rust trust boundary).

use std::str::FromStr;

use email_address::EmailAddress;

use crate::error::{AppError, AppResult};

/// Upper bound aligned with `RegisterSchema` / `LoginSchema` (`password.max(256)`).
const MAX_PASSWORD_CHARS: usize = 256;

/// Registration body — matches shared `RegisterSchema` shape.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterDto {
    pub email: String,
    pub password: String,
    pub name: Option<String>,
}

/// Login body — matches shared `LoginSchema` shape.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDto {
    pub email: String,
    pub password: String,
}

/// Validates [`RegisterDto`] using the same constraints as `RegisterSchema`.
///
/// # Errors
///
/// Returns [`AppError::InvalidInput`] when any field fails validation.
pub fn validate_register(dto: &RegisterDto) -> AppResult<()> {
    let _ = canonical_email(&dto.email)?;

    let pw_len = dto.password.chars().count();
    if !(8..=MAX_PASSWORD_CHARS).contains(&pw_len) {
        return Err(AppError::InvalidInput(format!(
            "password must be between 8 and {MAX_PASSWORD_CHARS} characters"
        )));
    }

    if let Some(ref name) = dto.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput(
                "name must be between 1 and 200 characters when provided".into(),
            ));
        }
        let len = name.chars().count();
        if len > 200 {
            return Err(AppError::InvalidInput(
                "name must be between 1 and 200 characters when provided".into(),
            ));
        }
    }

    Ok(())
}

/// Validates [`LoginDto`] using the same constraints as `LoginSchema`.
///
/// # Errors
///
/// Returns [`AppError::InvalidInput`] when any field fails validation.
pub fn validate_login(dto: &LoginDto) -> AppResult<()> {
    let _ = canonical_email(&dto.email)?;

    let pw_len = dto.password.chars().count();
    if !(1..=MAX_PASSWORD_CHARS).contains(&pw_len) {
        return Err(AppError::InvalidInput(format!(
            "password must be between 1 and {MAX_PASSWORD_CHARS} characters"
        )));
    }

    Ok(())
}

/// Canonical email for persistence and lookups (`trim` + ASCII lower-case) with
/// the same validity rules as `LoginSchema` / `RegisterSchema`.
///
/// # Errors
///
/// Returns [`AppError::InvalidInput`] when the address is empty, too long, or
/// not parseable.
pub fn canonical_email(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("email must not be empty".into()));
    }
    if trimmed.len() > 320 {
        return Err(AppError::InvalidInput("email is too long".into()));
    }
    let lower = trimmed.to_ascii_lowercase();
    let _ = EmailAddress::from_str(&lower)
        .map_err(|_| AppError::InvalidInput("email must be a valid address".into()))?;
    Ok(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_rejects_short_password() {
        let dto = RegisterDto {
            email: "a@b.co".into(),
            password: "short".into(),
            name: None,
        };
        assert!(validate_register(&dto).is_err());
    }

    #[test]
    fn login_accepts_normal_password() {
        let dto = LoginDto {
            email: "a@b.co".into(),
            password: "x".into(),
        };
        assert!(validate_login(&dto).is_ok());
    }
}
