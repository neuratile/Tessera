//! Health check IPC command.
//!
//! Per `rules.md` §4.2.1: returns system info + DB connectivity status.

use sqlx::SqlitePool;
use tauri::State;

use crate::services::health_service::{self, HealthStatus};

#[tauri::command]
pub async fn health_check(pool: State<'_, SqlitePool>) -> Result<HealthStatus, String> {
    health_service::check(&pool)
        .await
        .map_err(|e| e.to_string())
}
