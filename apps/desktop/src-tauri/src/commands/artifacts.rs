//! Artifact CRUD IPC commands — Phase 11 review surface.
//!
//! Per `rules.md` §4.2.1: thin command layer over `artifact_repo`.
//! Returns lightweight summaries for listing (no full markdown body)
//! and a separate `get_artifact` for the detail view, so the review
//! queue can render hundreds of items without shipping megabytes of
//! markdown over the IPC bridge.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::repositories::artifact_repo::{self, Artifact, ArtifactStatus};

/// Lightweight artifact projection for the review queue. Drops the
/// (potentially large) `content_md` and `structured_data` payloads.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub id: String,
    pub project_id: String,
    pub artifact_type: String,
    pub title: String,
    pub status: String,
    pub version: i64,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub provider: String,
    pub model: String,
}

impl From<Artifact> for ArtifactSummary {
    fn from(a: Artifact) -> Self {
        Self {
            id: a.id,
            project_id: a.project_id,
            artifact_type: a.artifact_type.as_ipc_str().to_string(),
            title: a.title,
            status: a.status.as_str().to_string(),
            version: a.version,
            parent_id: a.parent_id,
            created_at: a.created_at,
            updated_at: a.updated_at,
            provider: a.generation_metadata.provider,
            model: a.generation_metadata.model,
        }
    }
}

/// Full artifact payload — used by the review detail view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDetail {
    pub id: String,
    pub project_id: String,
    pub artifact_type: String,
    pub title: String,
    pub content_md: String,
    pub structured_data: serde_json::Value,
    pub status: String,
    pub version: i64,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl From<Artifact> for ArtifactDetail {
    fn from(a: Artifact) -> Self {
        Self {
            id: a.id,
            project_id: a.project_id,
            artifact_type: a.artifact_type.as_ipc_str().to_string(),
            title: a.title,
            content_md: a.content_md,
            structured_data: a.structured_data,
            status: a.status.as_str().to_string(),
            version: a.version,
            parent_id: a.parent_id,
            created_at: a.created_at,
            updated_at: a.updated_at,
            provider: a.generation_metadata.provider,
            model: a.generation_metadata.model,
            prompt_version: a.generation_metadata.prompt_version,
            input_tokens: a.generation_metadata.input_tokens,
            output_tokens: a.generation_metadata.output_tokens,
        }
    }
}

/// Default page size for the artifacts list endpoint. Keeps the IPC
/// payload bounded so the renderer cannot pull thousands of artifacts
/// (each carrying generation metadata) in a single round-trip.
const DEFAULT_PAGE_LIMIT: i64 = 100;
/// Hard cap on caller-supplied page sizes.
const MAX_PAGE_LIMIT: i64 = 1_000;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn list_artifacts(
    pool: State<'_, SqlitePool>,
    project_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ArtifactSummary>, String> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    artifact_repo::list_for_project(&pool, &project_id, limit, offset)
        .await
        .map(|v| v.into_iter().map(ArtifactSummary::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn get_artifact(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<ArtifactDetail, String> {
    artifact_repo::fetch(&pool, &id)
        .await
        .map(ArtifactDetail::from)
        .map_err(|e| e.to_string())
}

/// Lightweight version-chain entry — drives the version picker in
/// the artifact detail drawer. Excludes the markdown body so the
/// renderer can fetch the whole chain in one IPC round-trip without
/// paying the full content cost for every row.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVersionSummary {
    pub id: String,
    pub version: i64,
    pub status: String,
    pub title: String,
    pub created_at: String,
    pub parent_id: Option<String>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn list_artifact_versions(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<Vec<ArtifactVersionSummary>, String> {
    artifact_repo::list_version_chain(&pool, &id)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|r| ArtifactVersionSummary {
                    id: r.id,
                    version: r.version,
                    status: r.status.as_str().to_string(),
                    title: r.title,
                    created_at: r.created_at,
                    parent_id: r.parent_id,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn approve_artifact(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    artifact_repo::update_status(&pool, &id, ArtifactStatus::Approved)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn reject_artifact(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    artifact_repo::update_status(&pool, &id, ArtifactStatus::Rejected)
        .await
        .map_err(|e| e.to_string())
}
