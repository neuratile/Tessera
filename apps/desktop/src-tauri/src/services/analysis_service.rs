//! Analysis pipeline orchestrator.
//!
//! Per `rules.md` §4.2: ties file discovery, AST parsing, chunking,
//! embedding, and persistence into a single pipeline. No SQL — delegates
//! to repositories. No Tauri awareness — Phase 6 commands call this.
//!
//! Pipeline:
//! 1. Mark project `analyzing`.
//! 2. `file_discovery_service::discover` the project root.
//! 3. Persist discovered files via `project_file_repo`.
//! 4. For each source file: read → parse → chunk → embed.
//! 5. Persist chunks + embeddings via `chunk_repo`.
//! 6. Update project stats.
//! 7. Mark project `ready` (or `error` on failure).

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::providers::embeddings::EmbeddingProvider;
use crate::repositories::{chunk_repo, project_file_repo, project_repo};
use crate::services::file_discovery_service::{FileType, SourceLanguage};
use crate::services::{ast_service, chunking_service, file_discovery_service};

const EMBEDDING_BATCH_SIZE: usize = 32;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisOutcome {
    pub project_id: String,
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub chunks_created: usize,
    pub chunks_embedded: usize,
    pub total_size_bytes: u64,
}

/// Run the full analysis pipeline for a project.
///
/// # Errors
///
/// - `AppError::NotFound` if the project does not exist.
/// - `AppError::InvalidInput` if the root path is invalid.
/// - `AppError::LimitExceeded` if the project exceeds size/count caps.
/// - `AppError::Io` for filesystem errors.
/// - `AppError::Llm` for embedding failures.
/// - `AppError::Database` for persistence failures.
pub async fn analyze(
    pool: &SqlitePool,
    project_id: &str,
    embeddings: Arc<dyn EmbeddingProvider>,
) -> AppResult<AnalysisOutcome> {
    let project = project_repo::fetch(pool, project_id).await?;
    project_repo::update_status(pool, project_id, project_repo::ProjectStatus::Analyzing).await?;

    match run_pipeline(pool, project_id, &project.root_path, embeddings).await {
        Ok(outcome) => {
            project_repo::update_status(pool, project_id, project_repo::ProjectStatus::Ready)
                .await?;
            Ok(outcome)
        }
        Err(e) => {
            let _ =
                project_repo::update_status(pool, project_id, project_repo::ProjectStatus::Error)
                    .await;
            Err(e)
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn run_pipeline(
    pool: &SqlitePool,
    project_id: &str,
    root_path: &str,
    embeddings: Arc<dyn EmbeddingProvider>,
) -> AppResult<AnalysisOutcome> {
    project_file_repo::delete_for_project(pool, project_id).await?;

    let report = file_discovery_service::discover(root_path)?;
    info!(
        project_id,
        file_count = report.files.len(),
        total_size = report.total_size_bytes,
        "discovery complete"
    );

    let root = std::path::Path::new(root_path)
        .canonicalize()
        .map_err(|e| AppError::InvalidInput(format!("cannot canonicalize root: {e}")))?;

    let mut file_inserts = Vec::with_capacity(report.files.len());
    let mut language_counts: HashMap<String, u64> = HashMap::new();

    for f in &report.files {
        let abs_path = root.join(&f.relative_path);
        let content = std::fs::read(&abs_path).unwrap_or_default();
        let hash = format!("{:x}", Sha256::digest(&content));

        let lang_str = match f.language {
            SourceLanguage::JavaScript => Some("javascript".to_string()),
            SourceLanguage::TypeScript => Some("typescript".to_string()),
            SourceLanguage::Python => Some("python".to_string()),
            SourceLanguage::Unknown => None,
        };

        if let Some(ref l) = lang_str {
            *language_counts.entry(l.clone()).or_default() += 1;
        }

        file_inserts.push(project_file_repo::ProjectFileInsert {
            project_id: project_id.to_string(),
            path: f.relative_path.clone(),
            language: lang_str,
            size_bytes: i64::try_from(f.size_bytes).unwrap_or(i64::MAX),
            file_type: format!("{:?}", f.file_type).to_lowercase(),
            sha256: hash,
        });
    }

    let file_ids = project_file_repo::insert_batch(pool, file_inserts).await?;
    info!(project_id, file_count = file_ids.len(), "files persisted");

    let mut files_parsed: usize = 0;
    let mut all_chunk_inserts = Vec::new();

    for (idx, discovered) in report.files.iter().enumerate() {
        if discovered.file_type != FileType::Source && discovered.file_type != FileType::Test {
            continue;
        }
        if discovered.language == SourceLanguage::Unknown {
            continue;
        }

        let abs_path = root.join(&discovered.relative_path);
        let source = match std::fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(e) => {
                warn!(path = %discovered.relative_path, error = %e, "skip non-utf8 file");
                continue;
            }
        };

        let parsed = match ast_service::parse(&source, discovered.language) {
            Ok(p) => p,
            Err(e) => {
                warn!(path = %discovered.relative_path, error = %e, "AST parse failed, skipping");
                continue;
            }
        };
        files_parsed += 1;

        let chunks = chunking_service::chunk_source(&source, &parsed);
        let file_id = &file_ids[idx];

        for chunk in chunks {
            all_chunk_inserts.push((project_id.to_string(), file_id.clone(), chunk));
        }
    }

    info!(
        project_id,
        files_parsed,
        chunk_count = all_chunk_inserts.len(),
        "parsing + chunking complete"
    );

    let mut chunks_embedded: usize = 0;

    for batch_start in (0..all_chunk_inserts.len()).step_by(EMBEDDING_BATCH_SIZE) {
        let batch_end = (batch_start + EMBEDDING_BATCH_SIZE).min(all_chunk_inserts.len());
        let batch = &all_chunk_inserts[batch_start..batch_end];

        let texts: Vec<String> = batch.iter().map(|(_, _, c)| c.content.clone()).collect();
        let vectors = embeddings.embed(texts).await?;

        let dim = u32::try_from(embeddings.dimension()).unwrap_or(u32::MAX);
        let provider_name = format!("{}-{}", embeddings.name(), embeddings.model_id());

        let inserts: Vec<chunk_repo::ChunkInsert> = batch
            .iter()
            .zip(vectors)
            .map(|((pid, fid, chunk), embedding)| chunk_repo::ChunkInsert {
                project_id: pid.clone(),
                file_id: fid.clone(),
                chunk: chunk.clone(),
                embedding,
                embedding_dim: dim,
                embedding_provider: provider_name.clone(),
                embedding_model: embeddings.model_id().to_string(),
            })
            .collect();

        let ids = chunk_repo::insert_batch(pool, inserts).await?;
        chunks_embedded += ids.len();
    }

    let lang_json = serde_json::to_value(&language_counts)?;
    project_repo::update_stats(
        pool,
        project_id,
        i64::try_from(file_ids.len()).unwrap_or(i64::MAX),
        i64::try_from(report.total_size_bytes).unwrap_or(i64::MAX),
        &lang_json,
    )
    .await?;

    Ok(AnalysisOutcome {
        project_id: project_id.to_string(),
        files_discovered: file_ids.len(),
        files_parsed,
        chunks_created: all_chunk_inserts.len(),
        chunks_embedded,
        total_size_bytes: report.total_size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_outcome_serializes_to_camel_case() {
        let outcome = AnalysisOutcome {
            project_id: "p1".into(),
            files_discovered: 10,
            files_parsed: 5,
            chunks_created: 20,
            chunks_embedded: 20,
            total_size_bytes: 1024,
        };
        let json = serde_json::to_value(&outcome).expect("serialize");
        assert_eq!(json["filesDiscovered"], 10);
        assert_eq!(json["chunksEmbedded"], 20);
        assert_eq!(json["totalSizeBytes"], 1024);
    }
}
