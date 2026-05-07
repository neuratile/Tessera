//! Generation IPC command — wraps Phase 5 `generation_service`.
//!
//! Per `rules.md` §4.2.1: accepts IPC arguments, builds provider
//! instances, delegates to the generation service.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::providers::embeddings::OllamaEmbeddingProvider;
use crate::providers::factory;
use crate::repositories::artifact_repo::ArtifactType;
use crate::repositories::provider_config_repo;
use crate::services::generation_service::{
    self, GenerationDeps, GenerationRequest, StreamEvent, StreamSink,
};
use crate::services::provider_config_service;
use crate::utils::crypto::CryptoKey;

/// Tauri event channel that the renderer subscribes to for streaming
/// generation progress. Carries a `generationId` so concurrent
/// generations (e.g. user kicks off a second one before the first
/// completes) do not get cross-wired in the UI.
const GENERATION_EVENT: &str = "generation://event";

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateArgs {
    pub project_id: String,
    pub project_name: String,
    pub artifact_type: String,
    pub model: String,
    pub provider: String,
    #[serde(default)]
    pub scope_hint: String,
    #[serde(default)]
    pub project_summary: String,
    #[serde(default)]
    pub reviewer_feedback: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateResponse {
    pub generation_id: String,
    pub artifact_id: String,
    pub artifact_type: String,
    pub content_md: String,
    pub usage_input_tokens: u32,
    pub usage_output_tokens: u32,
}

/// Streaming-event payload emitted on the `generation://event` channel.
/// `kind` is the discriminator the renderer pivots on:
///
/// - `"text"`           — `delta` carries an incremental prose chunk
///   (rare; Phase 4 prompts force a tool call).
/// - `"tool_args"`      — `delta` carries a JSON fragment as the tool
///   call streams in. Renderer can concat for a partial preview.
/// - `"done"`           — final usage stats; no `delta`.
///
/// `generationId` is the Phase 13 correlation id that lets the UI
/// filter events when more than one generation is in flight.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEventPayload {
    pub generation_id: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn generate_artifact(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    config: State<'_, AppConfig>,
    crypto: State<'_, CryptoKey>,
    args: GenerateArgs,
) -> Result<GenerateResponse, String> {
    let artifact_type = parse_artifact_type(&args.artifact_type).map_err(|e| e.to_string())?;

    let row = provider_config_repo::fetch_active(&pool, DEFAULT_USER_ID, &args.provider)
        .await
        .map_err(|e| e.to_string())?;

    let provider_config =
        provider_config_service::build_provider_config(&crypto, &row).map_err(|e| e.to_string())?;

    let llm = factory::build_llm_provider(&provider_config).map_err(|e| e.to_string())?;

    let embeddings: Arc<dyn crate::providers::embeddings::EmbeddingProvider> = Arc::new(
        OllamaEmbeddingProvider::new(config.ollama_base_url.clone()).map_err(|e| e.to_string())?,
    );

    let request = GenerationRequest {
        project_id: args.project_id,
        project_name: args.project_name,
        artifact_type,
        model: args.model,
        scope_hint: args.scope_hint,
        project_summary: args.project_summary,
        reviewer_feedback: args.reviewer_feedback,
        parent_id: args.parent_id,
    };

    let deps = GenerationDeps {
        pool: &pool,
        llm,
        embeddings,
    };

    let generation_id = Uuid::new_v4().to_string();
    let sink = build_event_sink(app.clone(), generation_id.clone());

    let outcome = generation_service::generate(request, &deps, Some(sink))
        .await
        .map_err(|e| e.to_string())?;

    Ok(GenerateResponse {
        generation_id,
        artifact_id: outcome.artifact_id,
        artifact_type: outcome.artifact_type.as_str().to_string(),
        content_md: outcome.content_md,
        usage_input_tokens: outcome.usage_input_tokens,
        usage_output_tokens: outcome.usage_output_tokens,
    })
}

/// Build a `StreamSink` closure that fans `StreamEvent`s out as Tauri
/// events on the `generation://event` channel. Emit failures are
/// swallowed: the renderer disconnecting must not abort generation.
fn build_event_sink(app: AppHandle, generation_id: String) -> StreamSink {
    Box::new(move |event: StreamEvent| {
        let payload = match event {
            StreamEvent::Text(s) => StreamEventPayload {
                generation_id: generation_id.clone(),
                kind: "text",
                delta: Some(s),
                input_tokens: None,
                output_tokens: None,
            },
            StreamEvent::ToolArgsDelta(s) => StreamEventPayload {
                generation_id: generation_id.clone(),
                kind: "tool_args",
                delta: Some(s),
                input_tokens: None,
                output_tokens: None,
            },
            StreamEvent::Done {
                input_tokens,
                output_tokens,
            } => StreamEventPayload {
                generation_id: generation_id.clone(),
                kind: "done",
                delta: None,
                input_tokens: Some(input_tokens),
                output_tokens: Some(output_tokens),
            },
        };
        let _ = app.emit(GENERATION_EVENT, payload);
    })
}

fn parse_artifact_type(s: &str) -> Result<ArtifactType, crate::error::AppError> {
    ArtifactType::from_str_value(s)
        .ok_or_else(|| crate::error::AppError::InvalidInput(format!("unknown artifact_type `{s}`")))
}
