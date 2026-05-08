#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
// Thin command / repository / service pass-throughs propagate `AppError`
// uniformly; bullet-listing every variant per function adds noise without
// information. Critical services (e.g. `generation_service`) document
// errors in full where the variant set is non-obvious.
#![allow(clippy::missing_errors_doc)]

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
    let _sentry_guard = utils::telemetry::init_sentry(cfg.sentry_dsn.as_deref());
    utils::telemetry::init(&cfg.log_level).expect("failed to initialize tracing");

    tracing::info!(
        ollama_base_url = %cfg.ollama_base_url,
        sentry_configured = cfg.sentry_dsn.is_some(),
        "starting Testing IDE backend"
    );

    // Clone for `.setup`: `.manage(cfg)` moves the original into Tauri state.
    let cfg_for_db_path = cfg.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(cfg)
        .setup(move |app| {
            let db_path = db::resolve_app_db_path(app.handle(), &cfg_for_db_path)
                .map_err(|e| e.to_string())?;
            tracing::info!(db_path = %db_path.display(), "initializing database");
            let pool = tauri::async_runtime::block_on(db::init_pool_at(&db_path))
                .map_err(|e| e.to_string())?;
            app.manage(pool);

            let crypto_key =
                utils::crypto::CryptoKey::derive_from_secret(&cfg_for_db_path.jwt_secret);
            tracing::info!("encryption key derived from JWT secret");
            app.manage(crypto_key);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::init_db,
            // Auth commands (Phase 6)
            commands::auth::register,
            commands::auth::login,
            commands::auth::refresh_token,
            commands::auth::auth_me,
            // Project commands (Phase 4/5)
            commands::projects::create_project,
            commands::projects::list_projects,
            commands::projects::get_project,
            commands::projects::delete_project,
            // Analysis commands (Phase 3/4)
            commands::analysis::analyze_project,
            // Generation commands (Phase 5)
            commands::generation::generate_artifact,
            // Provider config commands (Phase 4)
            commands::providers::save_provider_config,
            commands::providers::list_provider_configs,
            commands::providers::delete_provider_config,
            commands::providers::test_provider_connection,
            commands::providers::list_ollama_models,
            // Artifact commands (Phase 11)
            commands::artifacts::list_artifacts,
            commands::artifacts::get_artifact,
            commands::artifacts::approve_artifact,
            commands::artifacts::reject_artifact,
            // Health / system commands
            commands::health::health_check,
            // Hardware detection command (Phase 8)
            commands::hardware::detect_hardware,
            // Ollama bootstrap command (Phase 7)
            commands::ollama::check_ollama_status,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Tauri application");
}
