//! Heal-check history repository — persistence for completed self-heal loops.
//!
//! Per `rules.md` §4.2 + §2.3 this module owns all SQL touching the
//! `heal_checks` and `heal_check_tests` tables (migration `0010_heal_checks.sql`)
//! and the row-shaped DTOs those tables map to. Services compose these; no
//! business logic lives here.
//!
//! Mirrors [`mutation_check_repo`](super::mutation_check_repo) and
//! [`flaky_check_repo`](super::flaky_check_repo) move-for-move
//! (plan/versions/v2/v2-feature-docs/V2_HARDENING.md §5.1): a heal's outcome
//! used to live only in memory. [`insert_check`] persists one header row plus
//! one row per involved test inside a single transaction (the codebase's batch
//! idiom — no N+1), so a check with N tests is one durable commit, not N.
//! [`list_checks`] / [`fetch_check`] read the history back for the trend UI.
//!
//! The DTOs live here (not in the service) so the repository stays the lowest
//! layer that knows the row shape — the service depends downward on these
//! types, never the other way around.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Default number of history rows [`list_checks`] returns when the caller does
/// not specify, and the hard ceiling it clamps any request down to. The UI
/// limit is only a hint — the backend re-clamps so a tampered IPC payload
/// cannot ask for an unbounded scan (mirrors the flaky / mutation clamp).
pub const DEFAULT_HISTORY_LIMIT: u32 = 20;
pub const MAX_HISTORY_LIMIT: u32 = 200;

/// Verdict of one test within a persisted heal (design §5.1). `snake_case` wire
/// form mirrors the sibling status enums and the Zod literals in
/// `heal.schema.ts`, and the TEXT stored in `heal_check_tests.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealTestStatus {
    /// Failed in an earlier attempt but passes in the landed attempt — the
    /// heal fixed it.
    Healed,
    /// Still failing in the landed attempt — a likely real source bug the heal
    /// could not paper over.
    StillFailing,
    /// Passed throughout. Reserved for forward-compat: a `HealResult` only
    /// carries the *failing* tests per attempt, so no writer emits this yet,
    /// but the decode path accepts it for a future heal that records the full
    /// final test list.
    Passed,
}

impl HealTestStatus {
    /// Stable string used in DB rows and IPC payloads. Matches the serde
    /// `snake_case` wire form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healed => "healed",
            Self::StillFailing => "still_failing",
            Self::Passed => "passed",
        }
    }

    /// Inverse of [`as_str`](Self::as_str). Returns `None` for any unrecognised
    /// string (corruption detection in the repository decode path).
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "healed" => Some(Self::Healed),
            "still_failing" => Some(Self::StillFailing),
            "passed" => Some(Self::Passed),
            _ => None,
        }
    }
}

/// One test involved in a heal, paired with its verdict (design §5.1). Mirrors
/// `HealTestRecordSchema`. `healed_at_attempt` is set only for `Healed` tests
/// (the attempt at which it first passed); `last_failure_message` is the most
/// recent captured failure (omitted from the wire payload when absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealTestRecord {
    pub name: String,
    pub status: HealTestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healed_at_attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_message: Option<String>,
}

/// One entry in an artifact's persisted heal history (design §5.1). A
/// lightweight header for the "Heal history" trend list — the per-test detail
/// is fetched on demand as a [`HealCheckRecord`]. Mirrors
/// `HealCheckSummarySchema`. `landed_run_id` is omitted (serde `None`) only if
/// that run row was later purged (the FK is `ON DELETE SET NULL`). `created_at`
/// is RFC-3339.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealCheckSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landed_run_id: Option<String>,
    pub landed_version_id: String,
    pub attempts: u32,
    pub healed_count: u32,
    pub still_failing_count: u32,
    pub final_passing: u32,
    pub final_total: u32,
    pub created_at: String,
}

/// A persisted heal check with its full per-test list (design §5.1). The detail
/// behind a [`HealCheckSummary`], re-rendered with the same per-test trail the
/// live result view derives. Mirrors `HealCheckRecordSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealCheckRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landed_run_id: Option<String>,
    pub landed_version_id: String,
    pub attempts: u32,
    pub healed_count: u32,
    pub still_failing_count: u32,
    pub final_passing: u32,
    pub final_total: u32,
    pub created_at: String,
    pub tests: Vec<HealTestRecord>,
}

/// Fields required to open a `heal_checks` header row. `landed_run_id` is the
/// run the heal settled on (nullable for symmetry with the column — the service
/// supplies it whenever the landed run was a real, completed run).
#[derive(Debug, Clone)]
pub struct HealCheckInsert {
    pub artifact_id: String,
    pub project_id: String,
    pub landed_run_id: Option<String>,
    pub landed_version_id: String,
    pub attempts: u32,
    pub healed_count: u32,
    pub still_failing_count: u32,
    pub final_passing: u32,
    pub final_total: u32,
}

/// Persist a completed heal check and its per-test verdicts in one transaction;
/// returns the new check id.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `artifact_id` or `project_id` is empty
///   after trim.
/// - [`AppError::Database`] for any SQLx-level failure (including a foreign-key
///   violation when the artifact / project / run does not exist).
pub async fn insert_check(
    pool: &SqlitePool,
    check: HealCheckInsert,
    tests: &[HealTestRecord],
) -> AppResult<String> {
    if check.artifact_id.trim().is_empty() {
        return Err(AppError::InvalidInput("heal check artifact_id is empty".into()));
    }
    if check.project_id.trim().is_empty() {
        return Err(AppError::InvalidInput("heal check project_id is empty".into()));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO heal_checks \
         (id, artifact_id, project_id, landed_run_id, landed_version_id, attempts, \
          healed_count, still_failing_count, final_passing, final_total, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&check.artifact_id)
    .bind(&check.project_id)
    .bind(check.landed_run_id.as_deref())
    .bind(&check.landed_version_id)
    .bind(i64::from(check.attempts))
    .bind(i64::from(check.healed_count))
    .bind(i64::from(check.still_failing_count))
    .bind(i64::from(check.final_passing))
    .bind(i64::from(check.final_total))
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    for test in tests {
        let test_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO heal_check_tests \
             (id, check_id, name, status, healed_at_attempt, last_failure_message, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&test_id)
        .bind(&id)
        .bind(&test.name)
        .bind(test.status.as_str())
        .bind(test.healed_at_attempt.map(i64::from))
        .bind(test.last_failure_message.as_deref())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(id)
}

/// List an artifact's heal-check history, newest first, capped at `limit`
/// (re-clamped to `[1, MAX_HISTORY_LIMIT]`). Returns header summaries only; the
/// per-test detail is fetched on demand via [`fetch_check`].
///
/// # Errors
///
/// [`AppError::Database`] for any SQLx-level failure.
pub async fn list_checks(
    pool: &SqlitePool,
    artifact_id: &str,
    limit: u32,
) -> AppResult<Vec<HealCheckSummary>> {
    let capped = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT id, landed_run_id, landed_version_id, attempts, healed_count, \
         still_failing_count, final_passing, final_total, created_at \
         FROM heal_checks WHERE artifact_id = ? \
         ORDER BY created_at DESC, id DESC LIMIT ?",
    )
    .bind(artifact_id)
    .bind(i64::from(capped))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(summary_from_row).collect())
}

/// Fetch one persisted check with its per-test verdicts, in insertion order.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no check matches `check_id`.
/// - [`AppError::Database`] when a stored `status` does not decode to a known
///   enum variant (corruption detection) or for any SQLx-level failure.
pub async fn fetch_check(pool: &SqlitePool, check_id: &str) -> AppResult<HealCheckRecord> {
    let header: Option<SummaryRow> = sqlx::query_as(
        "SELECT id, landed_run_id, landed_version_id, attempts, healed_count, \
         still_failing_count, final_passing, final_total, created_at \
         FROM heal_checks WHERE id = ?",
    )
    .bind(check_id)
    .fetch_optional(pool)
    .await?;

    let header = header.ok_or_else(|| AppError::NotFound(format!("heal check {check_id}")))?;
    let summary = summary_from_row(header);

    let test_rows: Vec<TestRow> = sqlx::query_as(
        "SELECT name, status, healed_at_attempt, last_failure_message \
         FROM heal_check_tests WHERE check_id = ? ORDER BY rowid ASC",
    )
    .bind(check_id)
    .fetch_all(pool)
    .await?;

    let tests = test_rows
        .into_iter()
        .map(|(name, status_s, healed_at_attempt, last_failure_message)| {
            let status = HealTestStatus::from_str_value(&status_s)
                .ok_or_else(|| decode_err("heal test status", &status_s))?;
            Ok(HealTestRecord {
                name,
                status,
                healed_at_attempt: healed_at_attempt.map(clamp_u32),
                last_failure_message,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(HealCheckRecord {
        id: summary.id,
        landed_run_id: summary.landed_run_id,
        landed_version_id: summary.landed_version_id,
        attempts: summary.attempts,
        healed_count: summary.healed_count,
        still_failing_count: summary.still_failing_count,
        final_passing: summary.final_passing,
        final_total: summary.final_total,
        created_at: summary.created_at,
        tests,
    })
}

/// Internal row shapes for sqlx decoding; aliased to keep the `query_as`
/// turbofish under clippy's `type_complexity` threshold (mirrors
/// `mutation_check_repo`).
type SummaryRow = (String, Option<String>, String, i64, i64, i64, i64, i64, String);
type TestRow = (String, String, Option<i64>, Option<String>);

fn summary_from_row(row: SummaryRow) -> HealCheckSummary {
    let (
        id,
        landed_run_id,
        landed_version_id,
        attempts,
        healed_count,
        still_failing_count,
        final_passing,
        final_total,
        created_at,
    ) = row;
    HealCheckSummary {
        id,
        landed_run_id,
        landed_version_id,
        attempts: clamp_u32(attempts),
        healed_count: clamp_u32(healed_count),
        still_failing_count: clamp_u32(still_failing_count),
        final_passing: clamp_u32(final_passing),
        final_total: clamp_u32(final_total),
        created_at,
    }
}

/// Saturating `i64 -> u32` for non-negative counters read back from `SQLite`.
/// The writer only ever binds values that originated as `u32`, so this never
/// clamps in practice; it stays total rather than panicking on an out-of-range
/// row.
fn clamp_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// Build the `Decode` error used when a stored status string does not map to a
/// known enum variant — surfaces as [`AppError::Database`].
fn decode_err(kind: &str, value: &str) -> AppError {
    AppError::Database(sqlx::Error::Decode(
        format!("unknown {kind} `{value}`").into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::repositories::artifact_repo::{self, ArtifactInsert, ArtifactType, GenerationMetadata};
    use crate::repositories::test_run_repo::{self, TestRunInsert, RUNNER_DOCKER_JS};
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-heal-hist-{}.db", Uuid::new_v4()))
    }

    /// Seed a project + a test-cases artifact + a landed run row so a
    /// `heal_checks` row satisfies all three foreign keys. Returns
    /// `(project_id, artifact_id, run_id)`.
    async fn seed(pool: &SqlitePool) -> (String, String, String) {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("seed project");

        let artifact_id = artifact_repo::insert(
            pool,
            ArtifactInsert {
                project_id: "p1".into(),
                artifact_type: ArtifactType::TestCases,
                title: "Cases v1".into(),
                content_md: "# Cases\n".into(),
                structured_data: serde_json::json!({ "cases": [] }),
                generation_metadata: GenerationMetadata {
                    provider: "ollama".into(),
                    model: "qwen2.5-coder:7b".into(),
                    prompt_version: "test_cases_v1".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    started_at: now.clone(),
                    completed_at: now.clone(),
                },
                parent_id: None,
            },
        )
        .await
        .expect("seed artifact");

        let run_id = test_run_repo::insert_run(
            pool,
            TestRunInsert {
                artifact_id: artifact_id.clone(),
                project_id: "p1".into(),
                runner: RUNNER_DOCKER_JS.into(),
            },
        )
        .await
        .expect("seed run");

        ("p1".to_string(), artifact_id, run_id)
    }

    async fn open_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        (pool, path)
    }

    fn healed(name: &str, at: u32, msg: &str) -> HealTestRecord {
        HealTestRecord {
            name: name.into(),
            status: HealTestStatus::Healed,
            healed_at_attempt: Some(at),
            last_failure_message: Some(msg.into()),
        }
    }

    fn still_failing(name: &str, msg: &str) -> HealTestRecord {
        HealTestRecord {
            name: name.into(),
            status: HealTestStatus::StillFailing,
            healed_at_attempt: None,
            last_failure_message: Some(msg.into()),
        }
    }

    #[tokio::test]
    async fn insert_rejects_empty_ids() {
        let (pool, path) = open_pool().await;
        let err = insert_check(
            &pool,
            HealCheckInsert {
                artifact_id: "   ".into(),
                project_id: "p1".into(),
                landed_run_id: None,
                landed_version_id: "v1".into(),
                attempts: 1,
                healed_count: 0,
                still_failing_count: 0,
                final_passing: 0,
                final_total: 0,
            },
            &[],
        )
        .await
        .expect_err("must reject empty artifact_id");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn check_round_trips_with_its_tests() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id, run_id) = seed(&pool).await;

        let tests = vec![
            healed("TC-A (sum)", 2, "expected 3 to equal 4"),
            still_failing("TC-B (tax)", "expected 19.99 to equal 20.00"),
        ];
        let check_id = insert_check(
            &pool,
            HealCheckInsert {
                artifact_id: artifact_id.clone(),
                project_id,
                landed_run_id: Some(run_id.clone()),
                landed_version_id: "v2".into(),
                attempts: 2,
                healed_count: 1,
                still_failing_count: 1,
                final_passing: 1,
                final_total: 2,
            },
            &tests,
        )
        .await
        .expect("insert check");

        let history = list_checks(&pool, &artifact_id, DEFAULT_HISTORY_LIMIT)
            .await
            .expect("list");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, check_id);
        assert_eq!(history[0].landed_run_id.as_deref(), Some(run_id.as_str()));
        assert_eq!(history[0].landed_version_id, "v2");
        assert_eq!(history[0].attempts, 2);
        assert_eq!(history[0].healed_count, 1);
        assert_eq!(history[0].still_failing_count, 1);
        assert_eq!(history[0].final_passing, 1);
        assert_eq!(history[0].final_total, 2);

        let record = fetch_check(&pool, &check_id).await.expect("fetch");
        assert_eq!(record.tests.len(), 2);
        assert_eq!(record.tests[0].status, HealTestStatus::Healed);
        assert_eq!(record.tests[0].healed_at_attempt, Some(2));
        assert_eq!(record.tests[1].status, HealTestStatus::StillFailing);
        assert_eq!(record.tests[1].healed_at_attempt, None);
        assert_eq!(
            record.tests[1].last_failure_message.as_deref(),
            Some("expected 19.99 to equal 20.00")
        );

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn fetch_missing_check_returns_not_found() {
        let (pool, path) = open_pool().await;
        let err = fetch_check(&pool, "no-such-check").await.expect_err("must error");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn deleting_the_landed_run_keeps_the_check_and_nulls_run_id() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id, run_id) = seed(&pool).await;

        let check_id = insert_check(
            &pool,
            HealCheckInsert {
                artifact_id: artifact_id.clone(),
                project_id,
                landed_run_id: Some(run_id.clone()),
                landed_version_id: "v1".into(),
                attempts: 1,
                healed_count: 1,
                still_failing_count: 0,
                final_passing: 2,
                final_total: 2,
            },
            &[healed("TC-A", 1, "boom")],
        )
        .await
        .expect("insert check");

        // Purging the landed run row must NOT delete the historical check
        // (ON DELETE SET NULL) — heal history outlives any one run.
        sqlx::query("DELETE FROM test_runs WHERE id = ?")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("delete run");

        let record = fetch_check(&pool, &check_id).await.expect("check survives");
        assert!(record.landed_run_id.is_none(), "run_id nulled out, not cascaded");
        assert_eq!(record.tests.len(), 1);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heal_test_status_round_trips() {
        for status in [HealTestStatus::Healed, HealTestStatus::StillFailing, HealTestStatus::Passed] {
            assert_eq!(HealTestStatus::from_str_value(status.as_str()), Some(status));
        }
        assert_eq!(HealTestStatus::from_str_value("bogus"), None);
    }
}
