//! Database liveness probe for the health check.
//!
//! Per `rules.md` §4.2: all SQL lives in the repository layer. The health
//! service asks here for a connectivity ping instead of touching `sqlx`
//! directly.

use sqlx::SqlitePool;

/// Returns `true` when the database answers a trivial `SELECT 1` ping.
///
/// The error is intentionally swallowed: the health check only needs the
/// boolean liveness signal, not the specific failure cause.
pub async fn is_reachable(pool: &SqlitePool) -> bool {
    sqlx::query("SELECT 1").execute(pool).await.is_ok()
}
