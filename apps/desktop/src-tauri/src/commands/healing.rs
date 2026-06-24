//! Self-heal IPC command — wraps the v2 `healing_service` orchestrator
//! (`plan/versions/v2/v2-feature-docs/SELF_HEALING_LOOP.md`).
//!
//! Per `rules.md` §4.2.1: a thin handler. It builds *both* dependency
//! bundles the loop needs — the [`GenerationDeps`] (LLM + embeddings, exactly
//! as `generate_artifact` does) and the [`SandboxDeps`] (per-language runner
//! factory + cancel registry, exactly as `run_test_sandbox` does) — then
//! delegates to [`healing_service::heal`] and maps the typed `AppError` to a
//! string at the IPC boundary. No business logic lives here.
//!
//! The opt-in gate is enforced inside each `sandbox_service::run` iteration,
//! not here, so an opted-out heal is rejected regardless of how it is invoked.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::providers::factory;
use crate::providers::runners::factory as runner_factory;
use crate::repositories::heal_check_repo::{HealCheckRecord, HealCheckSummary};
use crate::services::generation_service::GenerationDeps;
use crate::services::healing_service::{self, HealEvent, HealRequest, HealResult, HealSink};
use crate::services::sandbox_service::{RunRegistry, SandboxDeps};
use crate::services::{embedding_config_service, provider_config_service};
use crate::repositories::provider_config_repo;
use crate::utils::crypto::CryptoKey;

/// Tauri event channel the renderer subscribes to for per-attempt heal
/// progress. Carries a `healId` so a heal started while another is mid-flight
/// is not cross-wired in the UI (mirrors `generation://event`).
const HEAL_EVENT: &str = "heal://event";

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

/// IPC arguments for [`run_self_heal`]. Mirrors `HealRequestSchema`
/// (camelCase). `provider` selects the active LLM config (used to build the
/// generation deps); it is not part of the service-level [`HealRequest`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealArgs {
    pub artifact_id: String,
    pub max_attempts: u32,
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

/// Per-attempt progress payload emitted on the `heal://event` channel.
/// `kind` is always `"attempt"` in this first slice; the field is kept so the
/// renderer can pivot on future event kinds without a schema change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealEventPayload {
    pub heal_id: String,
    pub kind: &'static str,
    pub attempt: u32,
    pub passed: u32,
    pub failed: u32,
}

/// Run the bounded self-healing loop over a test-cases artifact and return the
/// final result.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) (Tauri IPC
/// requires `Result<T, String>`) only for a pre-flight failure before any
/// attempt runs (e.g. a blank artifact id, or the active provider config
/// cannot be resolved). A runner-level failure, a cancellation, or a
/// regeneration error is **not** an error here — it comes back as a
/// [`HealResult`] with `outcome: "error"`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn run_self_heal(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    registry: State<'_, RunRegistry>,
    config: State<'_, AppConfig>,
    crypto: State<'_, CryptoKey>,
    request: HealArgs,
) -> Result<HealResult, String> {
    // Generation deps: resolve the active provider into an LLM + embeddings,
    // exactly as `generate_artifact` does.
    let row = provider_config_repo::fetch_active(&pool, DEFAULT_USER_ID, &request.provider)
        .await
        .map_err(|e| e.to_string())?;
    let provider_config =
        provider_config_service::build_provider_config(&crypto, &row).map_err(|e| e.to_string())?;
    let llm = factory::build_llm_provider(&provider_config).map_err(|e| e.to_string())?;
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
        runner_factory: &runner_factory::runner_for,
        registry: &registry,
    };

    // Correlate progress events with the caller's `clientRunId` so the
    // renderer can match `heal://event`s to the heal it started (it never sees
    // a separate backend id). A blank id falls back to a fresh UUID.
    let heal_id = if request.client_run_id.trim().is_empty() {
        Uuid::new_v4().to_string()
    } else {
        request.client_run_id.clone()
    };

    let heal_request = HealRequest {
        artifact_id: request.artifact_id,
        max_attempts: request.max_attempts,
        opt_in_confirmed: request.opt_in_confirmed,
        client_run_id: request.client_run_id,
        model: request.model,
        project_id: request.project_id,
        project_name: request.project_name,
        scope_hint: request.scope_hint,
        project_summary: request.project_summary,
    };

    let sink = build_event_sink(app.clone(), heal_id);

    healing_service::heal(heal_request, &gen_deps, &sandbox_deps, Some(sink))
        .await
        .map_err(|e| e.to_string())
}

/// List an artifact's persisted self-heal history, newest first
/// (`plan/versions/v2/v2-feature-docs/V2_HARDENING.md` §5.1). Thin handler;
/// `limit` is re-clamped by the service/repository, so the UI value is only a
/// hint. Returns header summaries — the per-test detail is fetched on demand
/// via [`get_heal_check`].
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) for any
/// database-level failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn list_heal_checks(
    pool: State<'_, SqlitePool>,
    artifact_id: String,
    limit: u32,
) -> Result<Vec<HealCheckSummary>, String> {
    healing_service::list_heal_history(&pool, &artifact_id, limit)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch one persisted heal check with its per-test verdicts.
///
/// # Errors
///
/// Returns the stringified [`AppError`](crate::error::AppError) when no check
/// matches `check_id` (`NOT_FOUND`) or for any database-level failure.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn get_heal_check(
    pool: State<'_, SqlitePool>,
    check_id: String,
) -> Result<HealCheckRecord, String> {
    healing_service::get_heal_check(&pool, &check_id)
        .await
        .map_err(|e| e.to_string())
}

/// Build a [`HealSink`] that fans `HealEvent`s out as Tauri events on the
/// `heal://event` channel. Emit failures are swallowed — a disconnected
/// renderer must not abort the heal.
fn build_event_sink(app: AppHandle, heal_id: String) -> HealSink {
    Box::new(move |event: HealEvent| {
        let HealEvent::Attempt { attempt, passed, failed } = event;
        let payload = HealEventPayload {
            heal_id: heal_id.clone(),
            kind: "attempt",
            attempt,
            passed,
            failed,
        };
        let _ = app.emit(HEAL_EVENT, payload);
    })
}
