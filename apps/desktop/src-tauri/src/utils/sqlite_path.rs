//! Validation for optional `SQLite` file path overrides (`DB_PATH`).

use std::path::PathBuf;

use crate::error::{AppError, AppResult};

/// Upper bound accepted for user-supplied `DB_PATH` (env override).
pub const SQLITE_PATH_OVERRIDE_MAX_BYTES: usize = 4096;

/// Parse and validate `DB_PATH` supplied by configuration (trust boundary).
///
/// # Errors
///
/// Propagates [`AppError::Config`] when trimmed input is invalid.
pub fn parse_sqlite_path_override(raw: &str) -> AppResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config(
            "DB_PATH is set but empty after trimming".into(),
        ));
    }
    if trimmed.contains('\0') {
        return Err(AppError::Config(
            "DB_PATH must not contain NUL bytes".into(),
        ));
    }
    if trimmed.len() > SQLITE_PATH_OVERRIDE_MAX_BYTES {
        return Err(AppError::Config(format!(
            "DB_PATH exceeds maximum length ({SQLITE_PATH_OVERRIDE_MAX_BYTES} bytes)"
        )));
    }
    Ok(PathBuf::from(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_nul() {
        assert!(parse_sqlite_path_override("ok.db").is_ok());
        assert!(parse_sqlite_path_override("  ok.db  ").is_ok());
        assert!(parse_sqlite_path_override("").is_err());
        assert!(parse_sqlite_path_override("   ").is_err());
        assert!(parse_sqlite_path_override("bad\0.db").is_err());
    }
}
