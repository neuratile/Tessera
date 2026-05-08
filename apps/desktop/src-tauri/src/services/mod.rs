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
//! - [`generation_service`] (Phase 5) — orchestrator tying RAG +
//!   prompts + `LlmProvider` into one end-to-end artifact-production
//!   flow with token-budget enforcement and JSON-Schema validation.

pub mod analysis_service;
pub mod ast_service;
pub mod auth_service;
pub mod chunking_service;
pub mod file_discovery_service;
pub mod generation_service;
pub mod hardware_service;
pub mod health_service;
pub mod ollama_health_service;
pub mod project_service;
pub mod provider_config_service;
pub mod provider_connection_service;

#[cfg(test)]
pub mod ollama_probe_test_support;
