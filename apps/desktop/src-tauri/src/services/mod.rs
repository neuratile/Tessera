//! Business logic layer.
//!
//! Per `rules.md` §4.2: services contain domain logic, are testable in
//! isolation, hold no Tauri/IPC awareness, and never write SQL directly
//! (delegated to `repositories`). Services may call `providers` for
//! external integrations (LLM, embeddings).
//!
//! Sub-modules added in Phases 3 + 5: `file_discovery_service`,
//! `ast_service`, `chunking_service`, `generation_service`, `context_service`.
