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
        db_path = %cfg.db_path.display(),
        "starting Testing IDE backend"
    );

    // Bootstrap the DB synchronously on a single-thread runtime, then drop
    // it. Tauri commands run on Tauri's own tokio runtime — we do not need
    // to keep a second runtime alive for the lifetime of the app.
    let pool = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build bootstrap runtime")
        .block_on(db::init_pool(&cfg.database_url()))
        .expect("failed to initialize database");

    tauri::Builder::default()
        .manage(cfg)
        .manage(pool)
        .run(tauri::generate_context!())
        .expect("failed to start Tauri application");
}
