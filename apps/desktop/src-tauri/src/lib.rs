#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Testing IDE — Tauri backend library.
//!
//! Layered architecture per `rules.md` §4.2 (adapted for Rust/Tauri):
//! `commands` (Tauri IPC, replaces routes) → `services` → `repositories` → `db`.
//! Cross-cutting: `providers` (LLM/embeddings), `workers`, `prompts`, `utils`.

/// Entry point invoked from `main.rs`. Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start. This is acceptable per
/// `rules.md` §2.2 (panic only on invariant violations — a failed runtime
/// init at startup is unrecoverable).
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to start Tauri application");
}
