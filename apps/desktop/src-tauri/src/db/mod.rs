//! Database connection pool and migration runner.
//!
//! `SQLite` via sqlx 0.8. The pool is created with `create_if_missing(true)`
//! so a fresh install boots without manual file creation. Migrations live
//! in `migrations/` and run on every startup via the `sqlx::migrate!` macro
//! — sqlx tracks applied versions in the `_sqlx_migrations` table, so this
//! is idempotent.
//!
//! sqlite-vec note: the `vec0` virtual table for embedding ANN search is
//! deferred to a Phase 3 migration. Loading sqlite-vec via static linking
//! requires registering an auto-extension before any sqlx connection
//! opens; the exact API surface is verified empirically in Phase 3 when
//! `repositories::chunk_repo` is implemented. Until then `code_chunks`
//! stores embeddings as `BLOB` (packed f32 vectors) — usable for brute-
//! force cosine similarity at MVP scale.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use tauri::{AppHandle, Runtime};

use crate::error::{AppError, AppResult};

/// Default pool size. Desktop workload is low-concurrency (one user, a few
/// background workers); five connections covers analysis + generation +
/// IPC commands without thrashing `SQLite`'s writer lock.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// Maximum length accepted for a `DB_PATH` override (env). Prevents trivial
/// IPC/env abuse from allocating huge strings.
const MAX_DB_PATH_ENV_BYTES: usize = 4096;

fn sqlite_options_with_pragmas(options: SqliteConnectOptions) -> SqliteConnectOptions {
    options
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5))
}

async fn connect_pool_and_migrate(
    options: SqliteConnectOptions,
    log_target: &str,
) -> AppResult<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(DEFAULT_MAX_CONNECTIONS)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(AppError::Migration)?;

    tracing::info!(database = %log_target, "database pool ready");
    Ok(pool)
}

/// Build a `SQLite` pool from the configured database URL, creating the file
/// if necessary and applying all pending migrations.
///
/// Sets pragmas for desktop usage:
///   * `journal_mode = WAL` — concurrent readers + single writer
///   * `synchronous = NORMAL` — safe with WAL, faster than FULL
///   * `foreign_keys = ON` — enforce FK constraints (off by default)
///   * `busy_timeout = 5000` — wait 5s on contended writes
///
/// # Errors
///
/// Returns `AppError::Database` on connection failure or
/// `AppError::Migration` if any migration fails to apply.
pub async fn init_pool(database_url: &str) -> AppResult<SqlitePool> {
    let options = sqlite_options_with_pragmas(
        SqliteConnectOptions::from_str(database_url)
            .map_err(AppError::Database)?
            .create_if_missing(true),
    );
    let log_line = redact_url(database_url);
    connect_pool_and_migrate(options, &log_line).await
}

/// Build a pool against a path on disk. Uses sqlx file options (not a URI string)
/// so paths with spaces, Unicode, and Windows drive letters stay correct and safe.
///
/// # Errors
///
/// Same conditions as [`init_pool`].
pub async fn init_pool_at(path: &Path) -> AppResult<SqlitePool> {
    let options = sqlite_options_with_pragmas(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    );
    let log_line = redact_url(&path.display().to_string());
    connect_pool_and_migrate(options, &log_line).await
}

/// Parse and validate `DB_PATH` from the environment (trust boundary).
fn validated_env_db_path(raw: &str) -> AppResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config(
            "DB_PATH is set but empty after trimming".into(),
        ));
    }
    if trimmed.contains('\0') {
        return Err(AppError::Config("DB_PATH must not contain NUL bytes".into()));
    }
    if trimmed.len() > MAX_DB_PATH_ENV_BYTES {
        return Err(AppError::Config(format!(
            "DB_PATH exceeds maximum length ({MAX_DB_PATH_ENV_BYTES} bytes)"
        )));
    }
    Ok(PathBuf::from(trimmed))
}

/// Resolve the SQLite file path for this process: `DB_PATH` when set, otherwise
/// `<app_local_data_dir>/testing-ide.db` (Tauri desktop layout).
///
/// # Errors
///
/// Returns `AppError::Config` when the app data directory cannot be resolved,
/// or `AppError::Io` when the directory cannot be created.
pub fn resolve_app_db_path<R: Runtime>(handle: &AppHandle<R>) -> AppResult<PathBuf> {
    match std::env::var("DB_PATH") {
        Ok(value) if !value.trim().is_empty() => validated_env_db_path(&value),
        _ => {
            let dir = handle
                .path()
                .app_local_data_dir()
                .map_err(|e| AppError::Config(e.to_string()))?;
            std::fs::create_dir_all(&dir)?;
            Ok(dir.join(crate::config::DEFAULT_DB_FILENAME))
        }
    }
}

/// Strip query-string fragments from a `SQLite` URL before logging.
/// `SQLite` connection strings do not normally carry secrets, but logging
/// only the path keeps logs predictable across env-supplied auth params
/// (rules.md §5.4 — never log secrets).
fn redact_url(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn pool_creates_db_and_runs_migrations() {
        let tmp = env::temp_dir().join(format!("testing-ide-{}.db", uuid::Uuid::new_v4()));
        let pool = init_pool_at(&tmp)
            .await
            .expect("pool init must succeed on a fresh path");

        // Migration applied: users table exists and seed row landed.
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .expect("query must succeed");
        assert_eq!(row.0, 1, "seed user row expected");

        // FK enforcement is on (cannot insert orphan project).
        let bad = sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', 'no-such-user', 'x', '/tmp', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await;
        assert!(bad.is_err(), "FK violation must error");

        pool.close().await;
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn migrations_are_idempotent() {
        let tmp = env::temp_dir().join(format!("testing-ide-{}.db", uuid::Uuid::new_v4()));
        let pool1 = init_pool_at(&tmp).await.expect("first init");
        pool1.close().await;
        let pool2 = init_pool_at(&tmp).await.expect("second init must be no-op");
        pool2.close().await;

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn redact_url_strips_query() {
        assert_eq!(
            redact_url("sqlite:///tmp/x.db?mode=rwc&secret=foo"),
            "sqlite:///tmp/x.db"
        );
        assert_eq!(redact_url("sqlite:///tmp/x.db"), "sqlite:///tmp/x.db");
    }

    #[test]
    fn validated_env_db_path_rejects_nul_and_empty() {
        assert!(super::validated_env_db_path("ok.db").is_ok());
        assert!(super::validated_env_db_path("  ok.db  ").is_ok());
        assert!(super::validated_env_db_path("").is_err());
        assert!(super::validated_env_db_path("   ").is_err());
        assert!(super::validated_env_db_path("bad\0.db").is_err());
    }

    #[tokio::test]
    async fn pool_init_succeeds_when_path_has_spaces() {
        let dir = env::temp_dir().join(format!("testing ide {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_file = dir.join("app data.db");
        let pool = init_pool_at(&db_file)
            .await
            .expect("pool with spaces in path must init");
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
