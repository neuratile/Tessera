//! Flaky-check history repository — persistence for completed flaky checks.
//!
//! Per `rules.md` §4.2 + §2.3 this module owns all SQL touching the
//! `flaky_checks` and `flaky_check_tests` tables (migration
//! `0008_flaky_checks.sql`). Services call these functions; no business logic
//! lives here.
//!
//! Backs the "persisted flaky history" phase of flaky-test detection
//! (`plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md` §7): a check's
//! aggregate verdict used to live only in memory. [`insert_check`] persists
//! one header row plus one row per test verdict inside a single transaction
//! (the codebase's batch idiom — see `test_run_repo::insert_cases`), so a
//! check with N tests is one durable commit, not N. [`list_checks`] /
//! [`fetch_check`] read the history back for the trend UI.

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::providers::runners::{
    FlakyCheckRecord, FlakyCheckSummary, FlakyTestResult, TestVerdict,
};

/// Default number of history rows [`list_checks`] returns when the caller
/// does not specify, and the hard ceiling it clamps any request down to. The
/// UI limit is only a hint — the backend re-clamps so a tampered IPC payload
/// cannot ask for an unbounded scan (mirrors the flaky-run clamp philosophy).
pub const DEFAULT_HISTORY_LIMIT: u32 = 20;
pub const MAX_HISTORY_LIMIT: u32 = 200;

/// Fields required to open a `flaky_checks` header row. `run_id` is the
/// iteration-#1 run the check already persisted via the normal run path; it
/// is optional only for symmetry with the nullable column (the service always
/// supplies it).
#[derive(Debug, Clone)]
pub struct FlakyCheckInsert {
    pub artifact_id: String,
    pub project_id: String,
    pub run_id: Option<String>,
    pub total_runs: u32,
    pub flaky_count: u32,
    pub non_flaky_count: u32,
}

/// Persist a completed flaky check and its per-test verdicts in one
/// transaction; returns the new check id.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `artifact_id` or `project_id` is empty
///   after trim.
/// - [`AppError::Database`] for any SQLx-level failure (including a
///   foreign-key violation when the artifact / project / run does not exist).
pub async fn insert_check(
    pool: &SqlitePool,
    check: FlakyCheckInsert,
    tests: &[FlakyTestResult],
) -> AppResult<String> {
    if check.artifact_id.trim().is_empty() {
        return Err(AppError::InvalidInput("flaky check artifact_id is empty".into()));
    }
    if check.project_id.trim().is_empty() {
        return Err(AppError::InvalidInput("flaky check project_id is empty".into()));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO flaky_checks \
         (id, artifact_id, project_id, run_id, total_runs, flaky_count, non_flaky_count, \
          created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&check.artifact_id)
    .bind(&check.project_id)
    .bind(check.run_id.as_deref())
    .bind(i64::from(check.total_runs))
    .bind(i64::from(check.flaky_count))
    .bind(i64::from(check.non_flaky_count))
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    for test in tests {
        let test_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO flaky_check_tests \
             (id, check_id, name, verdict, pass_count, executed_count, total_runs, \
              sample_failure, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&test_id)
        .bind(&id)
        .bind(&test.name)
        .bind(test.verdict.as_str())
        .bind(i64::from(test.pass_count))
        .bind(i64::from(test.executed_count))
        .bind(i64::from(test.total_runs))
        .bind(test.sample_failure.as_deref())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(id)
}

/// List an artifact's flaky-check history, newest first, capped at `limit`
/// (re-clamped to `[1, MAX_HISTORY_LIMIT]`). Returns header summaries only;
/// the per-test detail is fetched on demand via [`fetch_check`].
///
/// # Errors
///
/// [`AppError::Database`] for any SQLx-level failure.
pub async fn list_checks(
    pool: &SqlitePool,
    artifact_id: &str,
    limit: u32,
) -> AppResult<Vec<FlakyCheckSummary>> {
    let capped = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT id, run_id, total_runs, flaky_count, non_flaky_count, created_at \
         FROM flaky_checks WHERE artifact_id = ? \
         ORDER BY created_at DESC, id DESC LIMIT ?",
    )
    .bind(artifact_id)
    .bind(i64::from(capped))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, run_id, total_runs, flaky_count, non_flaky_count, created_at)| {
            FlakyCheckSummary {
                id,
                run_id,
                total_runs: clamp_u32(total_runs),
                flaky_count: clamp_u32(flaky_count),
                non_flaky_count: clamp_u32(non_flaky_count),
                created_at,
            }
        })
        .collect())
}

/// Fetch one persisted check with its per-test verdicts, in insertion order.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no check matches `check_id`.
/// - [`AppError::Database`] when a stored `verdict` does not decode to a known
///   enum variant (corruption detection) or for any SQLx-level failure.
pub async fn fetch_check(pool: &SqlitePool, check_id: &str) -> AppResult<FlakyCheckRecord> {
    let header: Option<SummaryRow> = sqlx::query_as(
        "SELECT id, run_id, total_runs, flaky_count, non_flaky_count, created_at \
         FROM flaky_checks WHERE id = ?",
    )
    .bind(check_id)
    .fetch_optional(pool)
    .await?;

    let (id, run_id, total_runs, flaky_count, non_flaky_count, created_at) =
        header.ok_or_else(|| AppError::NotFound(format!("flaky check {check_id}")))?;

    let test_rows: Vec<TestRow> = sqlx::query_as(
        "SELECT name, verdict, pass_count, executed_count, total_runs, sample_failure \
         FROM flaky_check_tests WHERE check_id = ? ORDER BY rowid ASC",
    )
    .bind(check_id)
    .fetch_all(pool)
    .await?;

    let tests = test_rows
        .into_iter()
        .map(|(name, verdict_s, pass_count, executed_count, runs, sample_failure)| {
            let verdict = TestVerdict::from_str_value(&verdict_s)
                .ok_or_else(|| decode_err("test verdict", &verdict_s))?;
            Ok(FlakyTestResult {
                name,
                verdict,
                pass_count: clamp_u32(pass_count),
                executed_count: clamp_u32(executed_count),
                total_runs: clamp_u32(runs),
                sample_failure,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(FlakyCheckRecord {
        id,
        run_id,
        total_runs: clamp_u32(total_runs),
        flaky_count: clamp_u32(flaky_count),
        non_flaky_count: clamp_u32(non_flaky_count),
        created_at,
        tests,
    })
}

/// Internal row shapes for sqlx decoding; aliased to keep the `query_as`
/// turbofish under clippy's `type_complexity` threshold (mirrors the
/// `RunHeaderRow` alias in `test_run_repo`).
type SummaryRow = (String, Option<String>, i64, i64, i64, String);
type TestRow = (String, String, i64, i64, i64, Option<String>);

/// Saturating `i64 -> u32` for non-negative counters read back from
/// `SQLite`. The writer side only ever binds values that originated as
/// `u32`, so this never actually clamps in practice; it stays total rather
/// than panicking on a theoretically out-of-range row.
fn clamp_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// Build the `Decode` error used when a stored verdict string does not map to
/// a known enum variant — surfaces as [`AppError::Database`].
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
        std::env::temp_dir().join(format!("testing-ide-flaky-{}.db", Uuid::new_v4()))
    }

    /// Seed a project + a test-cases artifact and an iteration-#1 run row so a
    /// `flaky_checks` row satisfies all three foreign keys. Returns
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

    fn flaky_test(name: &str, verdict: TestVerdict, pass: u32, executed: u32, sample: Option<&str>) -> FlakyTestResult {
        FlakyTestResult {
            name: name.into(),
            verdict,
            pass_count: pass,
            executed_count: executed,
            total_runs: 5,
            sample_failure: sample.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn insert_rejects_empty_ids() {
        let (pool, path) = open_pool().await;
        let err = insert_check(
            &pool,
            FlakyCheckInsert {
                artifact_id: "   ".into(),
                project_id: "p1".into(),
                run_id: None,
                total_runs: 5,
                flaky_count: 0,
                non_flaky_count: 0,
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
            flaky_test("TC-LOGIN-01 ok", TestVerdict::StablePass, 5, 5, None),
            flaky_test("TC-CART-07 discount", TestVerdict::StableFail, 0, 5, Some("boom")),
            flaky_test("TC-CART-09 tax", TestVerdict::Flaky, 4, 5, Some("expected 19.99 to equal 20.00")),
        ];
        let check_id = insert_check(
            &pool,
            FlakyCheckInsert {
                artifact_id: artifact_id.clone(),
                project_id,
                run_id: Some(run_id.clone()),
                total_runs: 5,
                flaky_count: 1,
                non_flaky_count: 2,
            },
            &tests,
        )
        .await
        .expect("insert check");

        // Summary list reflects the header.
        let history = list_checks(&pool, &artifact_id, DEFAULT_HISTORY_LIMIT)
            .await
            .expect("list");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, check_id);
        assert_eq!(history[0].run_id.as_deref(), Some(run_id.as_str()));
        assert_eq!(history[0].total_runs, 5);
        assert_eq!(history[0].flaky_count, 1);
        assert_eq!(history[0].non_flaky_count, 2);

        // Detail carries the per-test verdicts in insertion order.
        let record = fetch_check(&pool, &check_id).await.expect("fetch");
        assert_eq!(record.tests.len(), 3);
        assert_eq!(record.tests[0].verdict, TestVerdict::StablePass);
        assert_eq!(record.tests[2].verdict, TestVerdict::Flaky);
        assert_eq!(record.tests[2].pass_count, 4);
        assert_eq!(
            record.tests[2].sample_failure.as_deref(),
            Some("expected 19.99 to equal 20.00")
        );

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn list_orders_newest_first_and_respects_limit() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id, run_id) = seed(&pool).await;

        // Three checks; created_at strings sort lexicographically, so vary them
        // explicitly via direct inserts to make ordering deterministic. Use the
        // `+00:00` offset form chrono's `to_rfc3339()` writes in production (not
        // a `Z` suffix) so the test sorts against the real stored format — the
        // two are semantically equal but order differently as plain strings.
        for (i, stamp) in [
            "2026-01-01T00:00:00+00:00",
            "2026-02-01T00:00:00+00:00",
            "2026-03-01T00:00:00+00:00",
        ]
        .iter()
        .enumerate()
        {
            sqlx::query(
                "INSERT INTO flaky_checks \
                 (id, artifact_id, project_id, run_id, total_runs, flaky_count, non_flaky_count, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(format!("c{i}"))
            .bind(&artifact_id)
            .bind(&project_id)
            .bind(&run_id)
            .bind(5_i64)
            .bind(i64::try_from(i).unwrap())
            .bind(0_i64)
            .bind(*stamp)
            .bind(*stamp)
            .execute(&pool)
            .await
            .expect("insert check row");
        }

        let all = list_checks(&pool, &artifact_id, DEFAULT_HISTORY_LIMIT)
            .await
            .expect("list");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].created_at, "2026-03-01T00:00:00+00:00", "newest first");
        assert_eq!(all[2].created_at, "2026-01-01T00:00:00+00:00");

        // A limit caps the result; the most recent rows survive.
        let limited = list_checks(&pool, &artifact_id, 1).await.expect("list limited");
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].created_at, "2026-03-01T00:00:00+00:00");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn fetch_missing_check_returns_not_found() {
        let (pool, path) = open_pool().await;
        let err = fetch_check(&pool, "no-such-check")
            .await
            .expect_err("must error");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn deleting_the_run_keeps_the_check_and_nulls_run_id() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id, run_id) = seed(&pool).await;

        let check_id = insert_check(
            &pool,
            FlakyCheckInsert {
                artifact_id: artifact_id.clone(),
                project_id,
                run_id: Some(run_id.clone()),
                total_runs: 5,
                flaky_count: 0,
                non_flaky_count: 1,
            },
            &[flaky_test("TC-A ok", TestVerdict::StablePass, 5, 5, None)],
        )
        .await
        .expect("insert check");

        // Purging the iteration-#1 run row must NOT delete the historical check
        // (ON DELETE SET NULL) — flakiness history outlives any one run.
        sqlx::query("DELETE FROM test_runs WHERE id = ?")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("delete run");

        let record = fetch_check(&pool, &check_id).await.expect("check survives");
        assert!(record.run_id.is_none(), "run_id nulled out, not cascaded");
        assert_eq!(record.tests.len(), 1);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
