//! Sandbox IPC command — wraps Phase 2 `sandbox_service`.
//!
//! Per `rules.md` §4.2.1: thin handler. It builds the concrete
//! [`DockerJsRunner`], delegates to the service (the sole orchestration
//! entry point), and maps the typed `AppError` to a string at the IPC
//! boundary. No business logic lives here.
//!
//! The opt-in gate (plan §3) is enforced in the service, not here, so the
//! backend rejects an opted-out run regardless of how it is invoked.

use std::sync::Arc;

use sqlx::SqlitePool;
use tauri::State;

use crate::providers::runners::docker_js::DockerJsRunner;
use crate::providers::runners::{RunRequest, RunResult, TestRunner};
use crate::services::sandbox_service::{self, RunRegistry, SandboxDeps};

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
    request: RunRequest,
) -> Result<RunResult, String> {
    let runner: Arc<dyn TestRunner> = Arc::new(DockerJsRunner::new());
    let deps = SandboxDeps {
        pool: &pool,
        runner,
        registry: &registry,
    };
    sandbox_service::run(request, &deps)
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
