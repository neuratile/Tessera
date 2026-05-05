#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Testing IDE — Tauri backend library.
//!
//! Layered architecture per `rules.md` §4.2 (adapted for Rust/Tauri):
//! `commands` (Tauri IPC, replaces routes) → `services` → `repositories` → `db`.
//! Cross-cutting: `providers` (LLM/embeddings), `workers`, `prompts`, `utils`.

pub mod auth;
pub mod commands;
pub mod config;
pub mod db;
pub mod error;
pub mod prompts;
pub mod providers;
pub mod repositories;
pub mod services;
pub mod utils;
pub mod workers;

use tauri::Manager;

/// Entry point invoked from `main.rs`. Loads configuration, initializes
/// structured logging, builds the database pool, then starts the Tauri
/// application with both stashed in the managed state container.
///
/// # Panics
///
/// Panics if configuration loading, logging init, database init, or the
/// Tauri runtime fails to start. This is acceptable per `rules.md` §2.2
/// (panic only on invariant violations — a failed startup is unrecoverable
/// and the panic message is the only useful signal because higher layers
/// may not yet be live).
pub fn run() {
    let cfg = config::AppConfig::from_env().expect("failed to load configuration");
    utils::telemetry::init(&cfg.log_level).expect("failed to initialize tracing");

    tracing::info!(
        ollama_base_url = %cfg.ollama_base_url,
        sentry_configured = cfg.sentry_dsn.is_some(),
        "starting Testing IDE backend"
    );

    // Clone for `.setup`: `.manage(cfg)` moves the original into Tauri state.
    let cfg_for_db_path = cfg.clone();

    tauri::Builder::default()
        .manage(cfg)
        .setup(move |app| {
            let db_path = db::resolve_app_db_path(app.handle(), &cfg_for_db_path)
                .map_err(|e| e.to_string())?;
            tracing::info!(db_path = %db_path.display(), "initializing database");
            let pool = tauri::async_runtime::block_on(db::init_pool_at(&db_path))
                .map_err(|e| e.to_string())?;
            app.manage(pool);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::init_db,
            commands::auth::register,
            commands::auth::login,
            commands::auth::refresh_token,
            commands::auth::auth_me,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Tauri application");
}
