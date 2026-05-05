//! Project CRUD IPC commands.
//!
//! Per `rules.md` §4.2.1: thin command layer — parse IPC input,
//! delegate to service, map errors to `String`.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::repositories::project_repo::Project;
use crate::services::project_service;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub file_count: i64,
    pub total_size_bytes: i64,
    pub status: String,
    pub language_breakdown: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Project> for ProjectResponse {
    fn from(p: Project) -> Self {
        Self {
            id: p.id,
            name: p.name,
            root_path: p.root_path,
            file_count: p.file_count,
            total_size_bytes: p.total_size_bytes,
            status: p.status.as_str().to_string(),
            language_breakdown: p.language_breakdown,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn create_project(
    pool: State<'_, SqlitePool>,
    name: String,
    root_path: String,
) -> Result<ProjectResponse, String> {
    project_service::create_project(&pool, name, root_path)
        .await
        .map(ProjectResponse::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_projects(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<ProjectResponse>, String> {
    project_service::list_projects(&pool)
        .await
        .map(|v| v.into_iter().map(ProjectResponse::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn get_project(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<ProjectResponse, String> {
    project_service::get_project(&pool, &id)
        .await
        .map(ProjectResponse::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn delete_project(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    project_service::delete_project(&pool, &id)
        .await
        .map_err(|e| e.to_string())
}
