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

/// Hard cap on the number of bytes sent to the embedding endpoint
/// per chunk. Ollama's default `nomic-embed-text` exposes a 2048-token
/// context window. In dense code blocks (e.g. minified JS assets), the
/// average token density can be extremely high (often under 1.2 characters
/// per token due to minimal spacing and punctuation). To safely fit within
/// the 2048-token limit without raising context-overflow errors, we set the
/// cap to 2,000 bytes.
///
/// Chunks longer than this are truncated *only* for the embedding
/// call. The full chunk content is still persisted so RAG search hits
/// return the complete symbol body to the LLM downstream.
const EMBEDDING_INPUT_CHAR_CAP: usize = 2_000;

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
        let content = tokio::fs::read(&abs_path).await.unwrap_or_default();
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

    // Zip discovered files with their assigned ids so the file_id
    // lookup stays in sync even when we skip non-source/non-test
    // files mid-loop.
    for (discovered, file_id) in report.files.iter().zip(file_ids.iter()) {
        if discovered.file_type != FileType::Source && discovered.file_type != FileType::Test {
            continue;
        }
        if discovered.language == SourceLanguage::Unknown {
            continue;
        }

        let abs_path = root.join(&discovered.relative_path);
        let source = match tokio::fs::read_to_string(&abs_path).await {
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

        // Truncate per-chunk content to fit the embedding model's
        // context window. Without this guard, a single oversize
        // chunk (e.g. a generated file or minified JS) makes Ollama
        // return `HTTP 400 the input length exceeds the context
        // length` and the whole analyze pipeline fails.
        let texts: Vec<String> = batch
            .iter()
            .map(|(_, _, c)| {
                if c.content.len() <= EMBEDDING_INPUT_CHAR_CAP {
                    c.content.clone()
                } else {
                    truncate_to_char_boundary(&c.content, EMBEDDING_INPUT_CHAR_CAP)
                }
            })
            .collect();
        let vectors = embeddings.embed(texts).await?;

        let dim = u32::try_from(embeddings.dimension()).unwrap_or(u32::MAX);
        let provider_name = embeddings.chunk_scope();

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

/// Truncate `s` to at most `max_bytes` *bytes* while keeping the cut
/// on a UTF-8 char boundary so the resulting string is still valid
/// UTF-8. `String::truncate` panics if it slices through a multi-byte
/// codepoint; this helper steps the cut backwards until it lands on a
/// boundary.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s[..cut].to_string()
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
