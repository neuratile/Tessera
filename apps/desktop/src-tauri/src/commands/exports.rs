//! Artifact export IPC commands (`plan/ARTIFACT_EXPORT.md` Phase 1).
//!
//! Thin per `rules.md` §4.2: validate input, delegate to
//! `services::export`, map errors to strings at the boundary. The
//! frontend obtains `dest_path` from a save dialog, but the service
//! re-validates it — any renderer code can invoke these commands.

use serde::Serialize;
use sqlx::SqlitePool;
use std::path::Path;
use tauri::State;

use crate::services::export::{self, ExportFormat};

/// Result of a file export. CSV/TSV exports of multi-section
/// artifacts can write sibling files, so the frontend toast lists
/// every path.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    pub files: Vec<String>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn export_artifact(
    pool: State<'_, SqlitePool>,
    artifact_id: String,
    format: ExportFormat,
    dest_path: String,
) -> Result<ExportOutcome, String> {
    export::export_artifact(&pool, &artifact_id, format, Path::new(&dest_path))
        .await
        .map(|paths| ExportOutcome {
            files: paths
                .into_iter()
                .map(|p| p.display().to_string())
                .collect(),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub async fn get_artifact_tsv(
    pool: State<'_, SqlitePool>,
    artifact_id: String,
) -> Result<String, String> {
    export::artifact_tsv(&pool, &artifact_id)
        .await
        .map_err(|e| e.to_string())
}
