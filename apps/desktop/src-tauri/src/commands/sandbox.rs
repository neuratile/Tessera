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

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::providers::factory as llm_factory;
use crate::providers::runners::factory;
use crate::providers::runners::mutation::{
    MutationCheckRecord, MutationCheckSummary, MutationResult,
};
use crate::providers::runners::{
    FlakyCheckRecord, FlakyCheckSummary, FlakyRunResult, RunRequest, RunResult,
};
use crate::repositories::provider_config_repo;
use crate::services::generation_service::GenerationDeps;
use crate::services::mutation_service::{
    self, ImproveEvent, ImproveRequest, ImproveResult, ImproveSink, MutationEvent, MutationSink,
};
use crate::services::sandbox_service::{self, RunRegistry, SandboxDeps};
use crate::services::{embedding_config_service, provider_config_service};
use crate::utils::crypto::CryptoKey;

/// Default single-user id used to resolve the active provider config — mirrors
/// `commands/healing.rs` (the app is single-user today).
const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

/// Tauri event channel the renderer subscribes to for per-mutant sweep
/// progress. Carries a `mutationId` so a sweep started while another is
/// mid-flight is not cross-wired in the UI (mirrors `heal://event`).
const MUTATION_EVENT: &str = "mutation://event";

/// Tauri event channel the renderer subscribes to for per-attempt "improve
/// coverage" progress. Carries an `improveId` so an improve started while
/// another is mid-flight is not cross-wired in the UI (mirrors `heal://event`).
const IMPROVE_EVENT: &str = "improve://event";

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

/// List an artifact's persisted flaky-check history, newest first
/// (`plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md` §7). Thin
/// handler; `limit` is re-clamped by the service/repository, so the UI value
/// is only a hint. Returns header summaries — the per-test detail is fetched
/// on demand via [`get_flaky_check`].
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) for any
/// database-level failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn list_flaky_checks(
    pool: State<'_, SqlitePool>,
    artifact_id: String,
    limit: u32,
) -> Result<Vec<FlakyCheckSummary>, String> {
    sandbox_service::list_flaky_history(&pool, &artifact_id, limit)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch one persisted flaky check with its per-test verdicts.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) when no check
/// matches `check_id` (`NOT_FOUND`) or for any database-level failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn get_flaky_check(
    pool: State<'_, SqlitePool>,
    check_id: String,
) -> Result<FlakyCheckRecord, String> {
    sandbox_service::get_flaky_check(&pool, &check_id)
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

/// Per-mutant progress payload emitted on the `mutation://event` channel
/// (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md` §5.4). `kind` is
/// always `"mutant"` in Stage 1; the field is kept so the renderer can pivot on
/// future event kinds without a schema change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationEventPayload {
    pub mutation_id: String,
    pub kind: &'static str,
    pub done: u32,
    pub total: u32,
}

/// Mutation-test a generated test-case artifact: score how many seeded bugs the
/// suite catches (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md`,
/// Stage 1). Thin handler mirroring [`run_test_sandbox_flaky`]; `maxMutants` is
/// re-clamped to `[1, 200]` by the engine, so the UI value is only a hint.
///
/// Streams per-mutant progress on `mutation://event`, correlated by the
/// caller's `clientRunId` so the renderer can match events to the sweep it
/// started (a blank id falls back to a fresh UUID — events simply go unmatched).
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) for the same
/// pre-flight failures as [`run_test_sandbox`] (opt-out, missing / wrong-type
/// artifact, no runnable files, DB error), **and** when the baseline suite is
/// not all-green, or the sweep is cancelled / the runner dies mid-sweep.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn run_mutation_test(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    registry: State<'_, RunRegistry>,
    crypto: State<'_, CryptoKey>,
    request: RunRequest,
    max_mutants: u32,
) -> Result<MutationResult, String> {
    let deps = SandboxDeps {
        pool: &pool,
        crypto: Some(&crypto),
        runner_factory: &factory::runner_for,
        registry: &registry,
    };

    let mutation_id = if request.client_run_id.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        request.client_run_id.clone()
    };
    let sink = build_mutation_sink(app.clone(), mutation_id);

    mutation_service::score(request, max_mutants, &deps, Some(sink))
        .await
        .map_err(|e| e.to_string())
}

/// List an artifact's persisted mutation-score history, newest first
/// (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md` §5.5). Thin handler;
/// `limit` is re-clamped by the service/repository. Returns header summaries —
/// the per-mutant detail is fetched on demand via [`get_mutation_check`].
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) for any
/// database-level failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn list_mutation_checks(
    pool: State<'_, SqlitePool>,
    artifact_id: String,
    limit: u32,
) -> Result<Vec<MutationCheckSummary>, String> {
    mutation_service::list_mutation_history(&pool, &artifact_id, limit)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch one persisted mutation check with its per-mutant verdicts.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) when no check
/// matches `check_id` (`NOT_FOUND`) or for any database-level failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn get_mutation_check(
    pool: State<'_, SqlitePool>,
    check_id: String,
) -> Result<MutationCheckRecord, String> {
    mutation_service::get_mutation_check(&pool, &check_id)
        .await
        .map_err(|e| e.to_string())
}

/// Build a [`MutationSink`] that fans `MutationEvent`s out as Tauri events on
/// the `mutation://event` channel. Emit failures are swallowed — a disconnected
/// renderer must not abort the sweep.
fn build_mutation_sink(app: AppHandle, mutation_id: String) -> MutationSink {
    Box::new(move |event: MutationEvent| {
        let MutationEvent::Mutant { done, total } = event;
        let payload = MutationEventPayload {
            mutation_id: mutation_id.clone(),
            kind: "mutant",
            done,
            total,
        };
        let _ = app.emit(MUTATION_EVENT, payload);
    })
}

/// IPC arguments for [`improve_coverage`]. Mirrors `ImproveRequestSchema`
/// (camelCase). `provider` selects the active LLM config used to build the
/// generation deps; it is not part of the service-level [`ImproveRequest`].
/// `maxMutants` / `maxAttempts` are hints — the backend re-clamps them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImproveArgs {
    pub artifact_id: String,
    pub max_attempts: u32,
    pub max_mutants: u32,
    pub opt_in_confirmed: bool,
    #[serde(default)]
    pub client_run_id: String,
    pub model: String,
    pub provider: String,
    pub project_id: String,
    pub project_name: String,
    #[serde(default)]
    pub scope_hint: String,
    #[serde(default)]
    pub project_summary: String,
}

/// Per-attempt progress payload emitted on the `improve://event` channel
/// (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md` §5.4). `kind` is
/// always `"attempt"`; the field is kept so the renderer can pivot on future
/// event kinds without a schema change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImproveEventPayload {
    pub improve_id: String,
    pub kind: &'static str,
    pub attempt: u32,
    pub score: f64,
}

/// Auto-generate tests that kill a suite's surviving mutants and re-score to
/// prove the lift (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md`,
/// Stage 2). Thin handler mirroring [`run_self_heal`](crate::commands::healing::run_self_heal):
/// it builds *both* the [`GenerationDeps`] (LLM + embeddings, as
/// `generate_artifact` does) and the [`SandboxDeps`] (runner factory + cancel
/// registry, as [`run_test_sandbox`] does), then delegates to
/// [`mutation_service::improve`].
///
/// Streams per-attempt progress on `improve://event`, correlated by the caller's
/// `clientRunId`. `maxAttempts` is re-clamped to `[1, 5]` and `maxMutants` to
/// `[1, 200]` by the service.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) only for a
/// pre-flight failure before any attempt runs: a blank artifact id, an
/// unresolvable provider config, or whatever the first score rejects up front
/// (opt-out, missing / wrong-type artifact, or a **red baseline**). A later
/// score failure or a regeneration error is **not** an error here — it comes
/// back as an [`ImproveResult`] with `outcome: "error"`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn improve_coverage(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    registry: State<'_, RunRegistry>,
    config: State<'_, AppConfig>,
    crypto: State<'_, CryptoKey>,
    request: ImproveArgs,
) -> Result<ImproveResult, String> {
    // Generation deps: resolve the active provider into an LLM + embeddings,
    // exactly as `generate_artifact` / `run_self_heal` do.
    let row = provider_config_repo::fetch_active(&pool, DEFAULT_USER_ID, &request.provider)
        .await
        .map_err(|e| e.to_string())?;
    let provider_config =
        provider_config_service::build_provider_config(&crypto, &row).map_err(|e| e.to_string())?;
    let llm = llm_factory::build_llm_provider(&provider_config).map_err(|e| e.to_string())?;
    let embeddings =
        embedding_config_service::resolve_provider(&pool, &crypto, &config.ollama_base_url)
            .await
            .map_err(|e| e.to_string())?;
    let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

    // Sandbox deps: per-language runner factory + cancel registry, exactly as
    // `run_test_sandbox` does.
    let sandbox_deps = SandboxDeps {
        pool: &pool,
        crypto: Some(&crypto),
        runner_factory: &factory::runner_for,
        registry: &registry,
    };

    let improve_id = if request.client_run_id.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        request.client_run_id.clone()
    };

    let improve_request = ImproveRequest {
        artifact_id: request.artifact_id,
        max_attempts: request.max_attempts,
        max_mutants: request.max_mutants,
        opt_in_confirmed: request.opt_in_confirmed,
        client_run_id: request.client_run_id,
        model: request.model,
        project_id: request.project_id,
        project_name: request.project_name,
        scope_hint: request.scope_hint,
        project_summary: request.project_summary,
    };

    let sink = build_improve_sink(app.clone(), improve_id);

    mutation_service::improve(improve_request, &gen_deps, &sandbox_deps, Some(sink))
        .await
        .map_err(|e| e.to_string())
}

/// Build an [`ImproveSink`] that fans `ImproveEvent`s out as Tauri events on the
/// `improve://event` channel. Emit failures are swallowed — a disconnected
/// renderer must not abort the loop.
fn build_improve_sink(app: AppHandle, improve_id: String) -> ImproveSink {
    Box::new(move |event: ImproveEvent| {
        let ImproveEvent::Attempt { attempt, score } = event;
        let payload = ImproveEventPayload {
            improve_id: improve_id.clone(),
            kind: "attempt",
            attempt,
            score,
        };
        let _ = app.emit(IMPROVE_EVENT, payload);
    })
}
