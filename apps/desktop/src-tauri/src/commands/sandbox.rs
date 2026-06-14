//! Sandbox IPC command — wraps Phase 2 `sandbox_service`.
//!
//! Per `rules.md` §4.2.1: thin handler. It hands the service the
//! per-language runner factory (`runners::factory::runner_for` — Python →
//! `docker-py`, JS/TS → `docker-js`), delegates to the service (the sole
//! orchestration entry point), and maps the typed `AppError` to a string
//! at the IPC boundary. No business logic lives here.
//!
//! The opt-in gate (plan §3) is enforced in the service, not here, so the
//! backend rejects an opted-out run regardless of how it is invoked.

use sqlx::SqlitePool;
use tauri::State;

use crate::providers::runners::factory;
use crate::providers::runners::{FlakyRunResult, RunRequest, RunResult};
use crate::services::sandbox_service::{self, RunRegistry, SandboxDeps};
use crate::utils::crypto::CryptoKey;

/// Execute a generated test-case artifact in the local Docker sandbox and
/// return the persisted result.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) (Tauri IPC
/// requires `Result<T, String>`) when the opt-in flag is off, the artifact
/// is missing / not a test-cases artifact / has no runnable files, or a
/// database call fails. A runner-level failure (Docker down, timeout) is
/// **not** an error here — it comes back as a `RunResult` with
/// `status: "error"`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn run_test_sandbox(
    pool: State<'_, SqlitePool>,
    registry: State<'_, RunRegistry>,
    crypto: State<'_, CryptoKey>,
    request: RunRequest,
) -> Result<RunResult, String> {
    let deps = SandboxDeps {
        pool: &pool,
        crypto: Some(&crypto),
        runner_factory: &factory::runner_for,
        registry: &registry,
    };
    sandbox_service::run(request, &deps)
        .await
        .map_err(|e| e.to_string())
}

/// Run a generated test-case artifact `runs` times in the local Docker
/// sandbox and classify each test as stable-pass / stable-fail / flaky
/// (`plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md`). Thin handler
/// mirroring [`run_test_sandbox`]; `runs` is re-clamped to `[2, 20]` in the
/// service, so the UI value is only a hint.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) for the same
/// pre-flight failures as [`run_test_sandbox`] (opt-out, missing / wrong-type
/// artifact, no runnable files, DB error). A runner-level failure or a
/// cancellation mid-check is **not** an error here — it comes back as a
/// [`FlakyRunResult`] carrying an `errorMessage` and no verdicts.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn run_test_sandbox_flaky(
    pool: State<'_, SqlitePool>,
    registry: State<'_, RunRegistry>,
    crypto: State<'_, CryptoKey>,
    request: RunRequest,
    runs: u32,
) -> Result<FlakyRunResult, String> {
    let deps = SandboxDeps {
        pool: &pool,
        crypto: Some(&crypto),
        runner_factory: &factory::runner_for,
        registry: &registry,
    };
    sandbox_service::run_flaky(request, runs, &deps)
        .await
        .map_err(|e| e.to_string())
}

/// Request cancellation of an in-flight sandbox run (UI Stop button). Fires
/// the run's cancellation token, which the runner races against — on a hit
/// the container is `docker kill`ed and the run finalizes as `cancelled`.
///
/// Returns `true` when a live run matched, `false` when the run already
/// finished or the id is unknown (both benign for the UI).
#[tauri::command]
#[must_use]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub fn cancel_test_sandbox(registry: State<'_, RunRegistry>, run_id: String) -> bool {
    sandbox_service::request_cancel(&registry, &run_id)
}
