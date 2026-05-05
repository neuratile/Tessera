//! Business logic layer.
//!
//! Per `rules.md` §4.2: services contain domain logic, are testable in
//! isolation, hold no Tauri/IPC awareness, and never write SQL directly
//! (delegated to `repositories`). Services may call `providers` for
//! external integrations (LLM, embeddings).
//!
//! Sub-modules:
//!
//! - [`file_discovery_service`] (Phase 3) — project-folder walk with
//!   `.gitignore` filtering, extension allow-list, and size caps.
//! - [`ast_service`] (Phase 3) — Tree-sitter parsing (JS / TS / Python)
//!   into typed declarations, imports, and exports.
//! - [`chunking_service`] (Phase 3) — semantic chunking at function /
//!   class boundaries, ready for embedding.
//!
//! Future Phase 3 / 5 modules: `generation_service`, `context_service`.

pub mod ast_service;
pub mod auth_service;
pub mod chunking_service;
pub mod file_discovery_service;
