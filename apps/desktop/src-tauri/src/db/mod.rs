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

use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};

/// Default pool size. Desktop workload is low-concurrency (one user, a few
/// background workers); five connections covers analysis + generation +
/// IPC commands without thrashing `SQLite`'s writer lock.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

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
    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(AppError::Database)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(DEFAULT_MAX_CONNECTIONS)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(AppError::Migration)?;

    tracing::info!(database_url = %redact_url(database_url), "database pool ready");
    Ok(pool)
}

/// Build a pool against a path on disk. Convenience wrapper around
/// [`init_pool`] for callers that already hold a `Path` instead of a URL.
///
/// # Errors
///
/// Same conditions as [`init_pool`].
pub async fn init_pool_at(path: &Path) -> AppResult<SqlitePool> {
    let normalized = path.display().to_string().replace('\\', "/");
    let url = format!("sqlite://{normalized}?mode=rwc");
    init_pool(&url).await
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
}
