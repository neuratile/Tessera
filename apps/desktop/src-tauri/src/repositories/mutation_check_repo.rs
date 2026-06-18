//! Mutation-check history repository — persistence for completed mutation
//! scores.
//!
//! Per `rules.md` §4.2 + §2.3 this module owns all SQL touching the
//! `mutation_checks` and `mutation_check_mutants` tables (migration
//! `0009_mutation_checks.sql`). Services call these functions; no business
//! logic lives here.
//!
//! Mirrors [`flaky_check_repo`](super::flaky_check_repo) move-for-move
//! (plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md §5.5): a check's score
//! used to live only in memory. [`insert_check`] persists one header row plus
//! one row per mutant inside a single transaction (the codebase's batch idiom —
//! no N+1), so a check with N mutants is one durable commit, not N.
//! [`list_checks`] / [`fetch_check`] read the history back for the trend UI.

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::providers::runners::mutation::{
    Mutant, MutantResult, MutantStatus, MutationCheckRecord, MutationCheckSummary,
};

/// Default number of history rows [`list_checks`] returns when the caller does
/// not specify, and the hard ceiling it clamps any request down to. The UI
/// limit is only a hint — the backend re-clamps so a tampered IPC payload
/// cannot ask for an unbounded scan (mirrors the flaky-history clamp).
pub const DEFAULT_HISTORY_LIMIT: u32 = 20;
pub const MAX_HISTORY_LIMIT: u32 = 200;

/// Fields required to open a `mutation_checks` header row. `baseline_run_id` is
/// the green baseline run the score was measured against; optional only for
/// symmetry with the nullable column (the service always supplies it).
#[derive(Debug, Clone)]
pub struct MutationCheckInsert {
    pub artifact_id: String,
    pub project_id: String,
    pub baseline_run_id: Option<String>,
    pub score: f64,
    pub killed: u32,
    pub survived: u32,
    pub errored: u32,
    pub total: u32,
    pub dropped_count: u32,
}

/// Persist a completed mutation check and its per-mutant verdicts in one
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
    check: MutationCheckInsert,
    mutants: &[MutantResult],
) -> AppResult<String> {
    if check.artifact_id.trim().is_empty() {
        return Err(AppError::InvalidInput("mutation check artifact_id is empty".into()));
    }
    if check.project_id.trim().is_empty() {
        return Err(AppError::InvalidInput("mutation check project_id is empty".into()));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO mutation_checks \
         (id, artifact_id, project_id, baseline_run_id, score, killed, survived, errored, \
          total, dropped_count, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&check.artifact_id)
    .bind(&check.project_id)
    .bind(check.baseline_run_id.as_deref())
    .bind(check.score)
    .bind(i64::from(check.killed))
    .bind(i64::from(check.survived))
    .bind(i64::from(check.errored))
    .bind(i64::from(check.total))
    .bind(i64::from(check.dropped_count))
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    for entry in mutants {
        let mutant_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO mutation_check_mutants \
             (id, check_id, file, line, operator_id, original, replacement, status, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&mutant_id)
        .bind(&id)
        .bind(&entry.mutant.file)
        .bind(i64::from(entry.mutant.line))
        .bind(&entry.mutant.operator_id)
        .bind(&entry.mutant.original)
        .bind(&entry.mutant.replacement)
        .bind(entry.status.as_str())
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(id)
}

/// List an artifact's mutation-check history, newest first, capped at `limit`
/// (re-clamped to `[1, MAX_HISTORY_LIMIT]`). Returns header summaries only; the
/// per-mutant detail is fetched on demand via [`fetch_check`].
///
/// # Errors
///
/// [`AppError::Database`] for any SQLx-level failure.
pub async fn list_checks(
    pool: &SqlitePool,
    artifact_id: &str,
    limit: u32,
) -> AppResult<Vec<MutationCheckSummary>> {
    let capped = limit.clamp(1, MAX_HISTORY_LIMIT);
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT id, baseline_run_id, score, killed, survived, errored, total, dropped_count, created_at \
         FROM mutation_checks WHERE artifact_id = ? \
         ORDER BY created_at DESC, id DESC LIMIT ?",
    )
    .bind(artifact_id)
    .bind(i64::from(capped))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(summary_from_row).collect())
}

/// Fetch one persisted check with its per-mutant verdicts, in insertion order.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no check matches `check_id`.
/// - [`AppError::Database`] when a stored `status` does not decode to a known
///   enum variant (corruption detection) or for any SQLx-level failure.
pub async fn fetch_check(pool: &SqlitePool, check_id: &str) -> AppResult<MutationCheckRecord> {
    let header: Option<SummaryRow> = sqlx::query_as(
        "SELECT id, baseline_run_id, score, killed, survived, errored, total, dropped_count, created_at \
         FROM mutation_checks WHERE id = ?",
    )
    .bind(check_id)
    .fetch_optional(pool)
    .await?;

    let header = header.ok_or_else(|| AppError::NotFound(format!("mutation check {check_id}")))?;
    let summary = summary_from_row(header);

    let mutant_rows: Vec<MutantRow> = sqlx::query_as(
        "SELECT file, line, operator_id, original, replacement, status \
         FROM mutation_check_mutants WHERE check_id = ? ORDER BY rowid ASC",
    )
    .bind(check_id)
    .fetch_all(pool)
    .await?;

    let mutants = mutant_rows
        .into_iter()
        .map(|(file, line, operator_id, original, replacement, status_s)| {
            let status = MutantStatus::from_str_value(&status_s)
                .ok_or_else(|| decode_err("mutant status", &status_s))?;
            Ok(MutantResult {
                mutant: Mutant {
                    file,
                    line: clamp_u32(line),
                    operator_id,
                    original,
                    replacement,
                    // Byte offsets are not persisted — they are only meaningful
                    // against the exact baseline source and are unused by the
                    // history UI, which renders file:line + original→replacement.
                    byte_start: 0,
                    byte_end: 0,
                },
                status,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(MutationCheckRecord {
        id: summary.id,
        baseline_run_id: summary.baseline_run_id,
        score: summary.score,
        killed: summary.killed,
        survived: summary.survived,
        errored: summary.errored,
        total: summary.total,
        dropped_count: summary.dropped_count,
        created_at: summary.created_at,
        mutants,
    })
}

/// Internal row shapes for sqlx decoding; aliased to keep the `query_as`
/// turbofish under clippy's `type_complexity` threshold (mirrors
/// `flaky_check_repo`).
type SummaryRow = (String, Option<String>, f64, i64, i64, i64, i64, i64, String);
type MutantRow = (String, i64, String, String, String, String);

fn summary_from_row(row: SummaryRow) -> MutationCheckSummary {
    let (id, baseline_run_id, score, killed, survived, errored, total, dropped_count, created_at) =
        row;
    MutationCheckSummary {
        id,
        baseline_run_id,
        score,
        killed: clamp_u32(killed),
        survived: clamp_u32(survived),
        errored: clamp_u32(errored),
        total: clamp_u32(total),
        dropped_count: clamp_u32(dropped_count),
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
        std::env::temp_dir().join(format!("testing-ide-mutation-{}.db", Uuid::new_v4()))
    }

    /// Seed a project + a test-cases artifact + a baseline run row so a
    /// `mutation_checks` row satisfies all three foreign keys. Returns
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

    fn mutant_result(file: &str, line: u32, original: &str, replacement: &str, status: MutantStatus) -> MutantResult {
        MutantResult {
            mutant: Mutant {
                file: file.into(),
                line,
                operator_id: "relational".into(),
                original: original.into(),
                replacement: replacement.into(),
                byte_start: 10,
                byte_end: 11,
            },
            status,
        }
    }

    #[tokio::test]
    async fn insert_rejects_empty_ids() {
        let (pool, path) = open_pool().await;
        let err = insert_check(
            &pool,
            MutationCheckInsert {
                artifact_id: "   ".into(),
                project_id: "p1".into(),
                baseline_run_id: None,
                score: 1.0,
                killed: 0,
                survived: 0,
                errored: 0,
                total: 0,
                dropped_count: 0,
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
    async fn check_round_trips_with_its_mutants() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id, run_id) = seed(&pool).await;

        let mutants = vec![
            mutant_result("cart.ts", 42, ">", ">=", MutantStatus::Survived),
            mutant_result("cart.ts", 51, "+", "-", MutantStatus::Killed),
            mutant_result("tax.ts", 18, "true", "false", MutantStatus::Errored),
        ];
        let check_id = insert_check(
            &pool,
            MutationCheckInsert {
                artifact_id: artifact_id.clone(),
                project_id,
                baseline_run_id: Some(run_id.clone()),
                score: 0.5,
                killed: 1,
                survived: 1,
                errored: 1,
                total: 3,
                dropped_count: 7,
            },
            &mutants,
        )
        .await
        .expect("insert check");

        let history = list_checks(&pool, &artifact_id, DEFAULT_HISTORY_LIMIT)
            .await
            .expect("list");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, check_id);
        assert_eq!(history[0].baseline_run_id.as_deref(), Some(run_id.as_str()));
        assert!((history[0].score - 0.5).abs() < f64::EPSILON);
        assert_eq!(history[0].killed, 1);
        assert_eq!(history[0].survived, 1);
        assert_eq!(history[0].errored, 1);
        assert_eq!(history[0].total, 3);
        assert_eq!(history[0].dropped_count, 7);

        let record = fetch_check(&pool, &check_id).await.expect("fetch");
        assert_eq!(record.mutants.len(), 3);
        assert_eq!(record.mutants[0].status, MutantStatus::Survived);
        assert_eq!(record.mutants[0].mutant.original, ">");
        assert_eq!(record.mutants[0].mutant.replacement, ">=");
        assert_eq!(record.mutants[2].status, MutantStatus::Errored);

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
    async fn deleting_the_baseline_run_keeps_the_check_and_nulls_run_id() {
        let (pool, path) = open_pool().await;
        let (project_id, artifact_id, run_id) = seed(&pool).await;

        let check_id = insert_check(
            &pool,
            MutationCheckInsert {
                artifact_id: artifact_id.clone(),
                project_id,
                baseline_run_id: Some(run_id.clone()),
                score: 1.0,
                killed: 2,
                survived: 0,
                errored: 0,
                total: 2,
                dropped_count: 0,
            },
            &[mutant_result("a.ts", 1, ">", ">=", MutantStatus::Killed)],
        )
        .await
        .expect("insert check");

        // Purging the baseline run row must NOT delete the historical check
        // (ON DELETE SET NULL) — mutation history outlives any one run.
        sqlx::query("DELETE FROM test_runs WHERE id = ?")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("delete run");

        let record = fetch_check(&pool, &check_id).await.expect("check survives");
        assert!(record.baseline_run_id.is_none(), "run_id nulled out, not cascaded");
        assert_eq!(record.mutants.len(), 1);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
