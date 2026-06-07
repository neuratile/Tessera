//! Embedding configuration IPC commands.
//!
//! Per `rules.md` §4.2.1 + §9: thin handlers over
//! `embedding_config_service`. API keys are encrypted before
//! persistence and never returned in plaintext over IPC
//! (`plan/EMBEDDING_PROVIDER_SELECT.md` §6.1).

use serde::Deserialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::config::AppConfig;
use crate::providers::embeddings::presets::{self, EmbeddingPreset};
use crate::services::embedding_config_service::{
    self, EmbeddingConfigView, IndexStatus, TestEmbeddingResult,
};
use crate::utils::crypto::CryptoKey;

/// IPC payload for `save_embedding_config` / `test_embedding_connection`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveEmbeddingConfigArgs {
    pub provider: String,
    pub model: String,
    pub dimension: u32,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

impl From<SaveEmbeddingConfigArgs> for embedding_config_service::SaveEmbeddingConfigArgs {
    fn from(args: SaveEmbeddingConfigArgs) -> Self {
        Self {
            provider: args.provider,
            model: args.model,
            dimension: args.dimension,
            base_url: args.base_url,
            api_key: args.api_key,
        }
    }
}

/// The active embedding config (or the implicit local-Ollama default).
#[tauri::command]
pub async fn get_embedding_config(
    pool: State<'_, SqlitePool>,
) -> Result<EmbeddingConfigView, String> {
    embedding_config_service::get_active_view(&pool)
        .await
        .map_err(|e| e.to_string())
}

/// Persist the embedding selection and mark it active.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn save_embedding_config(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    args: SaveEmbeddingConfigArgs,
) -> Result<EmbeddingConfigView, String> {
    embedding_config_service::save_config(&pool, &crypto, args.into())
        .await
        .map_err(|e| e.to_string())
}

/// Probe the given settings: embed one string, report latency and the
/// model's native dimension (the UI auto-fills its dimension field).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn test_embedding_connection(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    config: State<'_, AppConfig>,
    args: SaveEmbeddingConfigArgs,
) -> Result<TestEmbeddingResult, String> {
    embedding_config_service::test_connection(
        &pool,
        &crypto,
        &config.ollama_base_url,
        args.into(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Curated model presets — the Settings UI renders these instead of
/// hardcoding model names.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn list_embedding_presets() -> Result<Vec<EmbeddingPreset>, String> {
    Ok(presets::PRESETS.to_vec())
}

/// Compare a project's chunk index against the active embedding config
/// (stale-index banner data source).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn get_index_status(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    config: State<'_, AppConfig>,
    project_id: String,
) -> Result<IndexStatus, String> {
    embedding_config_service::index_status(&pool, &crypto, &config.ollama_base_url, &project_id)
        .await
        .map_err(|e| e.to_string())
}
