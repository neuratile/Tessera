//! Test-case-result repository — persistence for the per-case
//! execution-outcome sidecar (`plan/TEST_CASE_TABLE.md` §4.1).
//!
//! Per `rules.md` §4.2 + §2.3 this module owns all SQL touching the
//! `test_case_results` table (migration `0007_test_case_results.sql`).
//! Services and commands call these functions; no business logic lives
//! here.
//!
//! The table is the mutable half of the 9-column Test Case view:
//! columns 8–9 (Actual output / Result and remarks) are owned by a
//! human tester or the sandbox runner, not the LLM. Rows are keyed by
//! `(artifact_id, case_id)` with a `UNIQUE` constraint, so writes are
//! upserts — a manual edit and a later sandbox run overwrite the same
//! row (last writer wins; `source` records which produced it).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Execution outcome of one test case. Mirrors the `result` column and
/// the `TestCaseResultResultSchema` Zod literals in
/// `packages/shared/src/schemas/test-case-result.schema.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCaseResultStatus {
    Pass,
    Fail,
    Blocked,
    NotRun,
}

impl TestCaseResultStatus {
    /// Stable string stored in the `result` column / sent over IPC.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Blocked => "blocked",
            Self::NotRun => "not_run",
        }
    }

    /// Inverse of [`as_str`], used when decoding stored rows.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            "blocked" => Some(Self::Blocked),
            "not_run" => Some(Self::NotRun),
            _ => None,
        }
    }
}

/// Who wrote the current outcome. Mirrors the `source` column and the
/// `TestCaseResultSourceSchema` Zod literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCaseResultSource {
    Manual,
    Sandbox,
}

impl TestCaseResultSource {
    /// Stable string stored in the `source` column / sent over IPC.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Sandbox => "sandbox",
        }
    }

    /// Inverse of [`as_str`], used when decoding stored rows.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(Self::Manual),
            "sandbox" => Some(Self::Sandbox),
            _ => None,
        }
    }
}

/// Fields a caller provides to upsert one outcome. `id`, `created_at`,
/// and `updated_at` are managed by the repository.
#[derive(Debug, Clone)]
pub struct TestCaseResultUpsert {
    pub artifact_id: String,
    pub case_id: String,
    pub actual_output: Option<String>,
    pub result: TestCaseResultStatus,
    pub remarks: Option<String>,
    pub source: TestCaseResultSource,
    /// The originating sandbox run for `source = sandbox` rows; `None`
    /// for manual edits.
    pub run_id: Option<String>,
}

/// One stored outcome, returned to the renderer. `#[serde(rename_all =
/// "camelCase")]` so the JSON wire shape matches the Zod mirror.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseResultRow {
    pub id: String,
    pub artifact_id: String,
    pub case_id: String,
    pub actual_output: Option<String>,
    pub result: TestCaseResultStatus,
    pub remarks: Option<String>,
    pub source: TestCaseResultSource,
    pub run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Insert or update the outcome for one `(artifact_id, case_id)` pair.
///
/// On conflict the existing row's `id` and `created_at` are preserved;
/// every mutable column is overwritten with the new values (last
/// writer wins).
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `artifact_id` or `case_id` is
///   empty after trim.
/// - [`AppError::Database`] for any SQLx-level failure (including a
///   foreign-key violation when the artifact or run does not exist).
pub async fn upsert(pool: &SqlitePool, row: &TestCaseResultUpsert) -> AppResult<()> {
    validate(row)?;
    let now = Utc::now().to_rfc3339();
    upsert_one(pool, row, &now).await
}

/// Upsert many outcomes inside a single transaction (one commit, not
/// one per row — `rules.md` §2.3, no N+1). No-op for an empty slice.
/// Used by the sandbox bridge to write every case of a run at once.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when any row has an empty
///   `artifact_id` / `case_id`.
/// - [`AppError::Database`] for any SQLx-level failure.
pub async fn batch_upsert(pool: &SqlitePool, rows: &[TestCaseResultUpsert]) -> AppResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    for row in rows {
        validate(row)?;
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    for row in rows {
        upsert_one(&mut *tx, row, &now).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// List every stored outcome for an artifact in one query (no N+1).
/// Returns rows in insertion order.
///
/// # Errors
///
/// - [`AppError::Database`] when a stored `result` / `source` column
///   does not decode to a known enum variant (corruption detection) or
///   for any SQLx-level failure.
pub async fn list_by_artifact(
    pool: &SqlitePool,
    artifact_id: &str,
) -> AppResult<Vec<TestCaseResultRow>> {
    let raw: Vec<ResultTuple> = sqlx::query_as(
        "SELECT id, artifact_id, case_id, actual_output, result, remarks, source, run_id, \
         created_at, updated_at \
         FROM test_case_results WHERE artifact_id = ? ORDER BY rowid ASC",
    )
    .bind(artifact_id)
    .fetch_all(pool)
    .await?;

    raw.into_iter()
        .map(|(id, artifact_id, case_id, actual_output, result_s, remarks, source_s, run_id, created_at, updated_at)| {
            let result = TestCaseResultStatus::from_str_value(&result_s)
                .ok_or_else(|| decode_err("test case result", &result_s))?;
            let source = TestCaseResultSource::from_str_value(&source_s)
                .ok_or_else(|| decode_err("test case result source", &source_s))?;
            Ok(TestCaseResultRow {
                id,
                artifact_id,
                case_id,
                actual_output,
                result,
                remarks,
                source,
                run_id,
                created_at,
                updated_at,
            })
        })
        .collect()
}

/// Shared upsert body. Runs against any `SQLx` executor so the single
/// and batch paths share one statement (pool for [`upsert`], the
/// transaction for [`batch_upsert`]).
async fn upsert_one<'e, E>(executor: E, row: &TestCaseResultUpsert, now: &str) -> AppResult<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO test_case_results \
         (id, artifact_id, case_id, actual_output, result, remarks, source, run_id, \
          created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(artifact_id, case_id) DO UPDATE SET \
          actual_output = excluded.actual_output, \
          result = excluded.result, \
          remarks = excluded.remarks, \
          source = excluded.source, \
          run_id = excluded.run_id, \
          updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(&row.artifact_id)
    .bind(&row.case_id)
    .bind(row.actual_output.as_deref())
    .bind(row.result.as_str())
    .bind(row.remarks.as_deref())
    .bind(row.source.as_str())
    .bind(row.run_id.as_deref())
    .bind(now)
    .bind(now)
    .execute(executor)
    .await?;
    Ok(())
}

fn validate(row: &TestCaseResultUpsert) -> AppResult<()> {
    if row.artifact_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "test case result artifact_id is empty".into(),
        ));
    }
    if row.case_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "test case result case_id is empty".into(),
        ));
    }
    Ok(())
}

/// Row shape for sqlx decoding; aliased to keep the `query_as`
/// turbofish under clippy's `type_complexity` threshold (mirrors the
/// `CaseRow` alias in `test_run_repo`).
type ResultTuple = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
);

/// Build the `Decode` error used when a stored enum string does not
/// map to a known variant — surfaces as [`AppError::Database`].
fn decode_err(kind: &str, value: &str) -> AppError {
    AppError::Database(sqlx::Error::Decode(
        format!("unknown {kind} `{value}`").into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::repositories::artifact_repo::{
        self, ArtifactInsert, ArtifactType, GenerationMetadata,
    };
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-tcr-{}.db", Uuid::new_v4()))
    }

    async fn open_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        (pool, path)
    }

    /// Seed a project + a test-cases artifact and return the artifact id
    /// so a `test_case_results` row satisfies its foreign key.
    async fn seed_artifact(pool: &SqlitePool) -> String {
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

        artifact_repo::insert(
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
                    prompt_version: "test_cases_v2".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    started_at: now.clone(),
                    completed_at: now.clone(),
                },
                parent_id: None,
            },
        )
        .await
        .expect("seed artifact")
    }

    #[tokio::test]
    async fn upsert_rejects_empty_ids() {
        let (pool, path) = open_pool().await;
        let err = upsert(
            &pool,
            &TestCaseResultUpsert {
                artifact_id: "  ".into(),
                case_id: "TC-A".into(),
                actual_output: None,
                result: TestCaseResultStatus::NotRun,
                remarks: None,
                source: TestCaseResultSource::Manual,
                run_id: None,
            },
        )
        .await
        .expect_err("must reject empty artifact_id");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn upsert_inserts_then_conflict_overwrites_same_row() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool).await;

        // First write: a manual not-run row.
        upsert(
            &pool,
            &TestCaseResultUpsert {
                artifact_id: artifact_id.clone(),
                case_id: "TC-LOGIN-01".into(),
                actual_output: Some("not yet run".into()),
                result: TestCaseResultStatus::NotRun,
                remarks: Some("manual draft".into()),
                source: TestCaseResultSource::Manual,
                run_id: None,
            },
        )
        .await
        .expect("first upsert");

        let after_insert = list_by_artifact(&pool, &artifact_id).await.expect("list");
        assert_eq!(after_insert.len(), 1);
        assert_eq!(after_insert[0].result, TestCaseResultStatus::NotRun);
        assert_eq!(after_insert[0].source, TestCaseResultSource::Manual);
        let original_id = after_insert[0].id.clone();

        // Second write to the same (artifact, case): a sandbox failure.
        upsert(
            &pool,
            &TestCaseResultUpsert {
                artifact_id: artifact_id.clone(),
                case_id: "TC-LOGIN-01".into(),
                actual_output: Some("expected 401, got 500".into()),
                result: TestCaseResultStatus::Fail,
                remarks: None,
                source: TestCaseResultSource::Sandbox,
                run_id: None,
            },
        )
        .await
        .expect("second upsert");

        let after_conflict = list_by_artifact(&pool, &artifact_id).await.expect("list");
        // Still one row — the UNIQUE constraint folded the second write
        // into the first (last writer wins).
        assert_eq!(after_conflict.len(), 1);
        assert_eq!(after_conflict[0].id, original_id, "row identity preserved");
        assert_eq!(after_conflict[0].result, TestCaseResultStatus::Fail);
        assert_eq!(after_conflict[0].source, TestCaseResultSource::Sandbox);
        assert_eq!(
            after_conflict[0].actual_output.as_deref(),
            Some("expected 401, got 500")
        );
        // The sandbox write cleared the manual remarks.
        assert_eq!(after_conflict[0].remarks, None);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn batch_upsert_is_no_op_for_empty_and_writes_all_rows() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool).await;

        batch_upsert(&pool, &[]).await.expect("empty batch ok");
        assert!(list_by_artifact(&pool, &artifact_id)
            .await
            .expect("list")
            .is_empty());

        let rows = vec![
            TestCaseResultUpsert {
                artifact_id: artifact_id.clone(),
                case_id: "TC-A".into(),
                actual_output: Some("All 2 assertions passed.".into()),
                result: TestCaseResultStatus::Pass,
                remarks: None,
                source: TestCaseResultSource::Sandbox,
                run_id: None,
            },
            TestCaseResultUpsert {
                artifact_id: artifact_id.clone(),
                case_id: "TC-B".into(),
                actual_output: Some("expected true to be false".into()),
                result: TestCaseResultStatus::Fail,
                remarks: None,
                source: TestCaseResultSource::Sandbox,
                run_id: None,
            },
        ];
        batch_upsert(&pool, &rows).await.expect("batch upsert");

        let stored = list_by_artifact(&pool, &artifact_id).await.expect("list");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].case_id, "TC-A");
        assert_eq!(stored[0].result, TestCaseResultStatus::Pass);
        assert_eq!(stored[1].case_id, "TC-B");
        assert_eq!(stored[1].result, TestCaseResultStatus::Fail);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn list_for_unknown_artifact_is_empty() {
        let (pool, path) = open_pool().await;
        let rows = list_by_artifact(&pool, "no-such-artifact")
            .await
            .expect("list");
        assert!(rows.is_empty());
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
