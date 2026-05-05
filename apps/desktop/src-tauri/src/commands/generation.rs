//! Generation IPC command — wraps Phase 5 `generation_service`.
//!
//! Per `rules.md` §4.2.1: accepts IPC arguments, builds provider
//! instances, delegates to the generation service.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

use crate::config::AppConfig;
use crate::providers::embeddings::OllamaEmbeddingProvider;
use crate::providers::factory;
use crate::repositories::artifact_repo::ArtifactType;
use crate::repositories::provider_config_repo;
use crate::services::generation_service::{self, GenerationDeps, GenerationOutcome, GenerationRequest};
use crate::services::provider_config_service;
use crate::utils::crypto::CryptoKey;

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
    pub artifact_id: String,
    pub artifact_type: String,
    pub content_md: String,
    pub usage_input_tokens: u32,
    pub usage_output_tokens: u32,
}

impl From<GenerationOutcome> for GenerateResponse {
    fn from(o: GenerationOutcome) -> Self {
        Self {
            artifact_id: o.artifact_id,
            artifact_type: o.artifact_type.as_str().to_string(),
            content_md: o.content_md,
            usage_input_tokens: o.usage_input_tokens,
            usage_output_tokens: o.usage_output_tokens,
        }
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn generate_artifact(
    pool: State<'_, SqlitePool>,
    config: State<'_, AppConfig>,
    crypto: State<'_, CryptoKey>,
    args: GenerateArgs,
) -> Result<GenerateResponse, String> {
    let artifact_type = parse_artifact_type(&args.artifact_type)
        .map_err(|e| e.to_string())?;

    let row = provider_config_repo::fetch_active(
        &pool,
        DEFAULT_USER_ID,
        &args.provider,
    )
    .await
    .map_err(|e| e.to_string())?;

    let provider_config = provider_config_service::build_provider_config(&crypto, &row)
        .map_err(|e| e.to_string())?;

    let llm = factory::build_llm_provider(&provider_config)
        .map_err(|e| e.to_string())?;

    let embeddings: Arc<dyn crate::providers::embeddings::EmbeddingProvider> = Arc::new(
        OllamaEmbeddingProvider::new(config.ollama_base_url.clone())
            .map_err(|e| e.to_string())?,
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

    generation_service::generate(request, &deps, None)
        .await
        .map(GenerateResponse::from)
        .map_err(|e| e.to_string())
}

fn parse_artifact_type(s: &str) -> Result<ArtifactType, crate::error::AppError> {
    ArtifactType::from_str_value(s)
        .ok_or_else(|| crate::error::AppError::InvalidInput(
            format!("unknown artifact_type `{s}`"),
        ))
}
