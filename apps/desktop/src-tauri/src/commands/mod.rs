//! Tauri IPC command handlers.
//!
//! Per `rules.md` §4.2 (adapted), this module replaces the `routes/` layer
//! prescribed for HTTP backends. Tauri commands are the IPC equivalent of
//! HTTP routes: they parse input, delegate to a service, and format the
//! response. No business logic lives here.
//!
//! # Tauri-isms
//!
//! `#[tauri::command]` requires owned argument types (e.g. `String`, not
//! `&str`) so the IPC bridge can deserialize without lifetime puzzles.
//! That trips clippy's `needless_pass_by_value` lint, which we silence
//! at the function level rather than tightening the lint globally — the
//! constraint is real and Tauri-imposed.
//!
//! Sub-modules: `auth`, `analysis`, `generation`, `hardware`, `health`,
//! `ollama`, `projects`, `providers`.

pub mod analysis;
pub mod artifacts;
pub mod auth;
pub mod generation;
pub mod hardware;
pub mod health;
pub mod ollama;
pub mod projects;
pub mod providers;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, State};

use crate::config::AppConfig;
use crate::db;

/// Upper bound on greeting name length from IPC (`DoS` / log noise hardening).
const MAX_GREET_NAME_CHARS: usize = 256;

/// Response returned by [`init_db`] so the frontend can display the resolved path.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitDbResponse {
    /// Absolute path to the `SQLite` database file.
    pub db_path: String,
    /// Always `true` when the command succeeds (pool is reachable).
    pub ok: bool,
}

/// Placeholder command to verify IPC wiring end-to-end.
#[tauri::command]
#[must_use]
#[allow(clippy::needless_pass_by_value)] // Tauri IPC requires owned argument types.
pub fn greet(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Hello! (from Testing IDE Rust backend)".to_string();
    }
    let safe: String = trimmed.chars().take(MAX_GREET_NAME_CHARS).collect();
    format!("Hello, {safe}! (from Testing IDE Rust backend)")
}

/// Upper bound on frontend-supplied log message length so a misbehaving
/// renderer cannot flood the Rust-side tracing subscriber.
const MAX_FRONTEND_LOG_CHARS: usize = 2_048;

/// Bridge frontend warnings / errors into the Rust-side tracing
/// subscriber. The renderer is forbidden from calling `console.*`
/// (rules.md "no console.log in frontend") so this is the supported
/// channel for surfacing browser-context failures (listener install
/// errors, unexpected event payloads, etc.) into the structured log
/// stream.
///
/// `level` accepts `"warn"` and `"error"`; any other value maps to
/// `warn` so the renderer cannot silently downgrade.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn frontend_log(level: String, source: String, message: String) {
    let safe_message: String = message.chars().take(MAX_FRONTEND_LOG_CHARS).collect();
    let safe_source: String = source.chars().take(MAX_FRONTEND_LOG_CHARS).collect();
    if level.as_str() == "error" {
        tracing::error!(
            source = %safe_source,
            origin = "frontend",
            "{safe_message}"
        );
    } else {
        tracing::warn!(
            source = %safe_source,
            origin = "frontend",
            "{safe_message}"
        );
    }
}

/// Confirms the database file location and that the managed pool can execute a query.
///
/// The database file and migrations are applied during app
/// [`setup`](tauri::Builder::setup); this command is safe to call
/// multiple times.
///
/// # Errors
///
/// Returns the stringified error message (Tauri IPC requires
/// `Result<T, String>`) when:
///
/// - [`db::resolve_app_db_path`] fails — typically `AppError::Config` if
///   the platform's app-data directory cannot be resolved or
///   `AppError::Io` if the directory cannot be created.
/// - The managed pool rejects the smoke-test query (pool dropped /
///   connection lost / `SQLite` corruption).
#[tauri::command]
pub async fn init_db(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    cfg: State<'_, AppConfig>,
) -> Result<InitDbResponse, String> {
    let path = db::resolve_app_db_path(&app, &cfg).map_err(|e| e.to_string())?;
    db::run_migrations(&pool).await.map_err(|e| e.to_string())?;
    sqlx::query("SELECT 1")
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(InitDbResponse {
        db_path: path.display().to_string(),
        ok: true,
    })
}
