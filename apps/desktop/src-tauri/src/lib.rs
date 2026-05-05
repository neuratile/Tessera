#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Testing IDE — Tauri backend library.
//!
//! Layered architecture per `rules.md` §4.2 (adapted for Rust/Tauri):
//! `commands` (Tauri IPC, replaces routes) → `services` → `repositories` → `db`.
//! Cross-cutting: `providers` (LLM/embeddings), `workers`, `prompts`, `utils`.

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

// `tauri::App::manage` is provided by the `Manager` trait — bring it in
// scope so `app.manage(...)` resolves at the call site below.
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
        "starting Testing IDE backend"
    );

    tauri::Builder::default()
        .manage(cfg)
        .setup(|app| {
            let db_path = db::resolve_app_db_path(app.handle()).map_err(|e| e.to_string())?;
            tracing::info!(db_path = %db_path.display(), "initializing database");
            let pool = tauri::async_runtime::block_on(db::init_pool_at(&db_path))
                .map_err(|e| e.to_string())?;
            app.manage(pool);

            let data_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
            let crypto_key = utils::crypto::CryptoKey::load_or_generate(data_dir)
                .map_err(|e| e.to_string())?;
            tracing::info!("encryption key loaded");
            app.manage(crypto_key);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::init_db,
            commands::projects::create_project,
            commands::projects::list_projects,
            commands::projects::get_project,
            commands::projects::delete_project,
            commands::analysis::analyze_project,
            commands::generation::generate_artifact,
            commands::providers::save_provider_config,
            commands::providers::list_provider_configs,
            commands::providers::delete_provider_config,
            commands::health::health_check,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Tauri application");
}
