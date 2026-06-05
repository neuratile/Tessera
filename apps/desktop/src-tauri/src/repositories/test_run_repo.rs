//! Test-run repository — persistence for sandboxed test executions.
//!
//! Per `rules.md` §4.2 + §2.3 this module owns all SQL touching the
//! `test_runs`, `test_run_cases`, and `test_run_coverage` tables
//! (migration `0004_test_runs.sql`). Services call these functions; no
//! business logic lives here.
//!
//! Phase 1 (contract slice) ships the persistence skeleton the Phase 2
//! `sandbox_service` orchestration will drive: open a run, append its
//! cases + coverage, finalize it, and read it back as a
//! [`RunResult`](crate::providers::runners::RunResult). Cases and coverage
//! are written inside a single transaction per collection (one commit, not
//! one commit per row) — the codebase's established batch idiom (see
//! `project_file_repo::insert_batch`), so a run with N assertions does not
//! become N independent durable writes (`rules.md` §2.3, no N+1).

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::providers::runners::{CoverageLine, RunResult, RunStatus, TestResult, TestStatus};

/// Runner identifier stored in `test_runs.runner` for the JS/TS Docker
/// runner. Mirrors the `runner` literal in the data model (plan §5).
pub const RUNNER_DOCKER_JS: &str = "docker-js";

/// Fields required to open a run row. The row starts in
/// [`RunStatus::Pending`]; [`finalize_run`] writes the terminal state.
#[derive(Debug, Clone)]
pub struct TestRunInsert {
    pub artifact_id: String,
    pub project_id: String,
    pub runner: String,
}

/// Open a new run in `pending` state and return its assigned id.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `artifact_id` or `project_id` is
///   empty after trim.
/// - [`AppError::Database`] for any SQLx-level failure (including a
///   foreign-key violation when the artifact or project does not exist).
pub async fn insert_run(pool: &SqlitePool, row: TestRunInsert) -> AppResult<String> {
    if row.artifact_id.trim().is_empty() {
        return Err(AppError::InvalidInput("test run artifact_id is empty".into()));
    }
    if row.project_id.trim().is_empty() {
        return Err(AppError::InvalidInput("test run project_id is empty".into()));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO test_runs \
         (id, artifact_id, project_id, status, runner, passed_count, failed_count, \
          created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 0, 0, ?, ?)",
    )
    .bind(&id)
    .bind(&row.artifact_id)
    .bind(&row.project_id)
    .bind(RunStatus::Pending.as_str())
    .bind(&row.runner)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(id)
}

/// Flip a run to [`RunStatus::Running`] and stamp `started_at`.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no run matches `run_id`.
/// - [`AppError::Database`] for SQLx-level failures.
pub async fn mark_running(pool: &SqlitePool, run_id: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE test_runs SET status = ?, started_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(RunStatus::Running.as_str())
    .bind(&now)
    .bind(&now)
    .bind(run_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("test run {run_id}")));
    }
    Ok(())
}

/// Terminal fields recorded when a run completes (or fails to complete).
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub passed_count: u32,
    pub failed_count: u32,
    pub duration_ms: u32,
    pub error_message: Option<String>,
}

/// Mark a run terminal: persist status, counts, duration, `finished_at`,
/// and any error message.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no run matches `run_id`.
/// - [`AppError::Database`] for SQLx-level failures.
pub async fn finalize_run(
    pool: &SqlitePool,
    run_id: &str,
    outcome: RunOutcome,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE test_runs SET status = ?, passed_count = ?, failed_count = ?, \
         duration_ms = ?, finished_at = ?, error_message = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(outcome.status.as_str())
    .bind(i64::from(outcome.passed_count))
    .bind(i64::from(outcome.failed_count))
    .bind(i64::from(outcome.duration_ms))
    .bind(&now)
    .bind(outcome.error_message.as_deref())
    .bind(&now)
    .bind(run_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("test run {run_id}")));
    }
    Ok(())
}

/// Append the executed assertions for a run. No-op for an empty slice.
/// All inserts share one transaction so the batch commits once.
///
/// # Errors
///
/// [`AppError::Database`] for SQLx-level failures (e.g. a foreign-key
/// violation when `run_id` does not exist).
pub async fn insert_cases(
    pool: &SqlitePool,
    run_id: &str,
    cases: &[TestResult],
) -> AppResult<()> {
    if cases.is_empty() {
        return Ok(());
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    for case in cases {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO test_run_cases \
             (id, run_id, name, status, duration_ms, failure_message, source_line, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(run_id)
        .bind(&case.name)
        .bind(case.status.as_str())
        .bind(i64::from(case.duration_ms))
        .bind(case.failure_message.as_deref())
        .bind(case.source_line.map(i64::from))
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Append per-line coverage for a run. No-op for an empty slice. All
/// inserts share one transaction so the batch commits once.
///
/// # Errors
///
/// [`AppError::Database`] for SQLx-level failures (e.g. a foreign-key
/// violation when `run_id` does not exist).
pub async fn insert_coverage(
    pool: &SqlitePool,
    run_id: &str,
    coverage: &[CoverageLine],
) -> AppResult<()> {
    if coverage.is_empty() {
        return Ok(());
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    for line in coverage {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO test_run_coverage \
             (id, run_id, file_path, line, hits, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(run_id)
        .bind(&line.file_path)
        .bind(i64::from(line.line))
        .bind(i64::from(line.hits))
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Read a run back as the IPC-facing [`RunResult`], joining its cases and
/// coverage. Cases come back in insertion order; coverage is sorted by
/// file then line so the editor can stream gutters deterministically.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no run matches `run_id`.
/// - [`AppError::Database`] when a stored `status` column does not decode
///   to a known enum variant (corruption detection) or for any SQLx-level
///   failure.
pub async fn fetch_run(pool: &SqlitePool, run_id: &str) -> AppResult<RunResult> {
    let header: Option<RunHeaderRow> = sqlx::query_as(
        "SELECT status, passed_count, failed_count, duration_ms, error_message \
         FROM test_runs WHERE id = ?",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;

    let (status_s, passed, failed, duration, error_message) =
        header.ok_or_else(|| AppError::NotFound(format!("test run {run_id}")))?;

    let status = RunStatus::from_str_value(&status_s).ok_or_else(|| decode_err("run status", &status_s))?;

    let case_rows: Vec<CaseRow> = sqlx::query_as(
        "SELECT name, status, duration_ms, failure_message, source_line \
         FROM test_run_cases WHERE run_id = ? ORDER BY rowid ASC",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    let tests = case_rows
        .into_iter()
        .map(|(name, status_s, duration_ms, failure_message, source_line)| {
            let status = TestStatus::from_str_value(&status_s)
                .ok_or_else(|| decode_err("test status", &status_s))?;
            Ok(TestResult {
                name,
                status,
                duration_ms: clamp_u32(duration_ms),
                failure_message,
                source_line: source_line.map(clamp_u32),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let cov_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT file_path, line, hits FROM test_run_coverage \
         WHERE run_id = ? ORDER BY file_path ASC, line ASC",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    let coverage = cov_rows
        .into_iter()
        .map(|(file_path, line, hits)| CoverageLine {
            file_path,
            line: clamp_u32(line),
            hits: clamp_u32(hits),
        })
        .collect();

    Ok(RunResult {
        run_id: run_id.to_string(),
        status,
        passed_count: clamp_u32(passed),
        failed_count: clamp_u32(failed),
        duration_ms: duration.map_or(0, clamp_u32),
        tests,
        coverage,
        error_message,
    })
}

/// Internal row shapes for sqlx decoding; aliased to keep the `query_as`
/// turbofish under clippy's `type_complexity` threshold (mirrors the
/// `ArtifactRow` alias in `artifact_repo`).
type RunHeaderRow = (String, i64, i64, Option<i64>, Option<String>);
type CaseRow = (String, String, i64, Option<String>, Option<i64>);

/// Saturating `i64 -> u32` for non-negative counters read back from
/// `SQLite`. The writer side only ever binds values that originated as
/// `u32`, so this never actually clamps in practice; it stays total
/// rather than panicking on the theoretically-possible out-of-range row.
fn clamp_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// Build the `Decode` error used when a stored status string does not map
/// to a known enum variant — surfaces as [`AppError::Database`].
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
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-run-{}.db", Uuid::new_v4()))
    }

    /// Seed a project + a test-cases artifact and return their ids so a
    /// `test_runs` row satisfies both foreign keys.
    async fn seed(pool: &SqlitePool) -> (String, String) {
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

        ("p1".to_string(), artifact_id)
    }

    async fn open_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        (pool, path)
    }

    #[tokio::test]
    async fn insert_run_rejects_empty_ids() {
        let (pool, path) = open_pool().await;
        let err = insert_run(
            &pool,
            TestRunInsert {
                artifact_id: "   ".into(),
                project_id: "p1".into(),
                runner: RUNNER_DOCKER_JS.into(),
            },
        )
        .await
        .expect_err("must reject empty artifact_id");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn full_run_round_trips_through_repo() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id) = seed(&pool).await;

        let run_id = insert_run(
            &pool,
            TestRunInsert {
                artifact_id: artifact_id.clone(),
                project_id: project_id.clone(),
                runner: RUNNER_DOCKER_JS.into(),
            },
        )
        .await
        .expect("insert run");

        // Pending immediately after open.
        let pending = fetch_run(&pool, &run_id).await.expect("fetch pending");
        assert_eq!(pending.status, RunStatus::Pending);
        assert!(pending.tests.is_empty());
        assert!(pending.coverage.is_empty());

        mark_running(&pool, &run_id).await.expect("mark running");

        let cases = vec![
            TestResult {
                name: "adds two numbers".into(),
                status: TestStatus::Passed,
                duration_ms: 10,
                failure_message: None,
                source_line: None,
            },
            TestResult {
                name: "rejects bad input".into(),
                status: TestStatus::Failed,
                duration_ms: 4,
                failure_message: Some("expected 2 to equal 3".into()),
                source_line: Some(42),
            },
        ];
        insert_cases(&pool, &run_id, &cases).await.expect("insert cases");

        let coverage = vec![
            CoverageLine { file_path: "src/add.ts".into(), line: 1, hits: 3 },
            CoverageLine { file_path: "src/add.ts".into(), line: 2, hits: 0 },
        ];
        insert_coverage(&pool, &run_id, &coverage)
            .await
            .expect("insert coverage");

        finalize_run(
            &pool,
            &run_id,
            RunOutcome {
                status: RunStatus::Failed,
                passed_count: 1,
                failed_count: 1,
                duration_ms: 350,
                error_message: None,
            },
        )
        .await
        .expect("finalize");

        let result = fetch_run(&pool, &run_id).await.expect("fetch final");
        assert_eq!(result.run_id, run_id);
        assert_eq!(result.status, RunStatus::Failed);
        assert_eq!(result.passed_count, 1);
        assert_eq!(result.failed_count, 1);
        assert_eq!(result.duration_ms, 350);

        assert_eq!(result.tests.len(), 2);
        assert_eq!(result.tests[0].name, "adds two numbers");
        assert_eq!(result.tests[0].status, TestStatus::Passed);
        assert_eq!(result.tests[1].status, TestStatus::Failed);
        assert_eq!(result.tests[1].failure_message.as_deref(), Some("expected 2 to equal 3"));
        assert_eq!(result.tests[1].source_line, Some(42));

        assert_eq!(result.coverage.len(), 2);
        assert_eq!(result.coverage[0].line, 1);
        assert_eq!(result.coverage[0].hits, 3);
        assert_eq!(result.coverage[1].hits, 0); // uncovered line preserved

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn empty_batches_are_no_ops() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id) = seed(&pool).await;
        let run_id = insert_run(
            &pool,
            TestRunInsert { artifact_id, project_id, runner: RUNNER_DOCKER_JS.into() },
        )
        .await
        .expect("insert run");

        insert_cases(&pool, &run_id, &[]).await.expect("empty cases ok");
        insert_coverage(&pool, &run_id, &[]).await.expect("empty coverage ok");

        let result = fetch_run(&pool, &run_id).await.expect("fetch");
        assert!(result.tests.is_empty());
        assert!(result.coverage.is_empty());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn fetch_missing_run_returns_not_found() {
        let (pool, path) = open_pool().await;
        let err = fetch_run(&pool, "no-such-run").await.expect_err("must error");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn finalize_missing_run_returns_not_found() {
        let (pool, path) = open_pool().await;
        let err = finalize_run(
            &pool,
            "no-such-run",
            RunOutcome {
                status: RunStatus::Error,
                passed_count: 0,
                failed_count: 0,
                duration_ms: 0,
                error_message: Some("boom".into()),
            },
        )
        .await
        .expect_err("must error");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
