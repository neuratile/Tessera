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
        OllamaEmbeddingProvider::new(config.ollama_base_url.clone()).map_err(|e| e.to_string())?,
    );

    analysis_service::analyze(&pool, &project_id, embeddings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn get_analysis_outcome(
    pool: State<'_, SqlitePool>,
    project_id: String,
) -> Result<Option<AnalysisOutcome>, String> {
    // Check if project exists
    let project = crate::repositories::project_repo::fetch(&pool, &project_id)
        .await
        .map_err(|e| e.to_string())?;

    if project.status != crate::repositories::project_repo::ProjectStatus::Ready {
        return Ok(None);
    }

    let files_discovered: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_files WHERE project_id = ?"
    )
    .bind(&project_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let files_parsed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ast_analyses WHERE file_id IN (SELECT id FROM project_files WHERE project_id = ?)"
    )
    .bind(&project_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let chunks_created: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM code_chunks WHERE project_id = ?"
    )
    .bind(&project_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let chunks_embedded: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM code_chunks WHERE project_id = ? AND embedding IS NOT NULL"
    )
    .bind(&project_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(Some(AnalysisOutcome {
        project_id,
        files_discovered: usize::try_from(files_discovered).unwrap_or(0),
        files_parsed: usize::try_from(files_parsed).unwrap_or(0),
        chunks_created: usize::try_from(chunks_created).unwrap_or(0),
        chunks_embedded: usize::try_from(chunks_embedded).unwrap_or(0),
        total_size_bytes: u64::try_from(project.total_size_bytes).unwrap_or(0),
    }))
}

