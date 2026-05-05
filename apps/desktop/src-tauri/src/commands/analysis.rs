//! Analysis pipeline IPC command.
//!
//! Per `rules.md` §4.2.1: triggers the full analysis pipeline for a
//! project. Builds the embedding provider from the active Ollama config
//! (default, no API key needed).

use std::sync::Arc;

use sqlx::SqlitePool;
use tauri::State;

use crate::config::AppConfig;
use crate::providers::embeddings::OllamaEmbeddingProvider;
use crate::services::analysis_service::{self, AnalysisOutcome};

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn analyze_project(
    pool: State<'_, SqlitePool>,
    config: State<'_, AppConfig>,
    project_id: String,
) -> Result<AnalysisOutcome, String> {
    let embeddings: Arc<dyn crate::providers::embeddings::EmbeddingProvider> = Arc::new(
        OllamaEmbeddingProvider::new(config.ollama_base_url.clone())
            .map_err(|e| e.to_string())?,
    );

    analysis_service::analyze(&pool, &project_id, embeddings)
        .await
        .map_err(|e| e.to_string())
}
