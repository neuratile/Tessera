//! Tauri IPC command handlers.
//!
//! Per `rules.md` §4.2 (adapted), this module replaces the `routes/` layer
//! prescribed for HTTP backends. Tauri commands are the IPC equivalent of
//! HTTP routes: they parse input, delegate to a service, and format the
//! response. No business logic lives here.
//!
//! Sub-modules added in Phase 6: `projects`, `analysis`, `generation`,
//! `providers`, `health`.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, State};

use crate::db;

/// Upper bound on greeting name length from IPC (DoS / log noise hardening).
const MAX_GREET_NAME_CHARS: usize = 256;

/// Response returned by [`init_db`] so the frontend can display the resolved path.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitDbResponse {
    /// Absolute path to the SQLite database file.
    pub db_path: String,
    /// Always `true` when the command succeeds (pool is reachable).
    pub ok: bool,
}

/// Placeholder command to verify IPC wiring end-to-end.
#[tauri::command]
pub fn greet(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Hello! (from Testing IDE Rust backend)".to_string();
    }
    let safe: String = trimmed.chars().take(MAX_GREET_NAME_CHARS).collect();
    format!("Hello, {safe}! (from Testing IDE Rust backend)")
}

/// Confirms the database file location and that the managed pool can execute a query.
///
/// The database file and migrations are applied during app [`setup`](tauri::Builder::setup);
/// this command is safe to call multiple times.
#[tauri::command]
pub async fn init_db(app: AppHandle, pool: State<'_, SqlitePool>) -> Result<InitDbResponse, String> {
    let path = db::resolve_app_db_path(&app).map_err(|e| e.to_string())?;
    sqlx::query("SELECT 1")
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(InitDbResponse {
        db_path: path.display().to_string(),
        ok: true,
    })
}
