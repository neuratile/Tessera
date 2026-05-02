//! Background task workers.
//!
//! Long-running jobs (project analysis, embedding generation, hierarchical
//! summarization) run as `tokio::task::spawn` workers here, decoupled from
//! the IPC command that triggered them. Progress streamed back to the UI
//! via `tauri::Window::emit`.
//!
//! Sub-modules added in Phase 3+: `analysis_worker`, `embedding_worker`.
