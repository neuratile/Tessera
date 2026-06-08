//! Test-case-result IPC commands — the manual-edit half of the
//! 9-column Test Case table (`plan/TEST_CASE_TABLE.md` §5, Phase 2).
//!
//! Per `rules.md` §4.2.1: thin handlers over `test_case_result_repo`
//! (the same direct-to-repo shape as `commands/artifacts.rs`). A tester
//! types Actual output / Result + remarks; those land here as manual
//! rows. The sandbox auto-fill path is separate — it folds run results
//! into the same table from `sandbox_service` with `source = sandbox`.

use sqlx::SqlitePool;
use tauri::State;

use crate::repositories::test_case_result_repo::{
    self, TestCaseResultRow, TestCaseResultSource, TestCaseResultStatus, TestCaseResultUpsert,
};

/// Manual upsert payload from the renderer. Mirrors
/// `UpsertTestCaseResultInputSchema` in `packages/shared/`. `source` is
/// always `manual` on this path (the sandbox path writes its own rows),
/// so it is not part of the wire shape.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTestCaseResultInput {
    pub artifact_id: String,
    pub case_id: String,
    pub actual_output: Option<String>,
    pub result: TestCaseResultStatus,
    pub remarks: Option<String>,
}

/// List every stored execution outcome for an artifact, so the table
/// can LEFT JOIN them onto the LLM cases on mount.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) (Tauri
/// IPC requires `Result<T, String>`) on any database failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn list_test_case_results(
    pool: State<'_, SqlitePool>,
    artifact_id: String,
) -> Result<Vec<TestCaseResultRow>, String> {
    test_case_result_repo::list_by_artifact(&pool, &artifact_id)
        .await
        .map_err(|e| e.to_string())
}

/// Upsert one manual outcome (Actual output / Result + remarks). A
/// later sandbox run on the same case overwrites the row; a manual edit
/// after a run overwrites it back — last writer wins (plan §4.1).
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) when
/// `artifactId` / `caseId` is empty or a database call fails.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn upsert_test_case_result(
    pool: State<'_, SqlitePool>,
    input: UpsertTestCaseResultInput,
) -> Result<(), String> {
    test_case_result_repo::upsert(
        &pool,
        &TestCaseResultUpsert {
            artifact_id: input.artifact_id,
            case_id: input.case_id,
            actual_output: input.actual_output,
            result: input.result,
            remarks: input.remarks,
            source: TestCaseResultSource::Manual,
            run_id: None,
        },
    )
    .await
    .map_err(|e| e.to_string())
}
