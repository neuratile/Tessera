//! Database access layer.
//!
//! Per `rules.md` §4.2 + §2.3: all SQL lives here. Services call
//! repositories; repositories call sqlx. Parameterized queries only —
//! never concatenate user input into SQL.
//!
//! Sub-modules:
//!
//! - [`chunk_repo`] (Phase 3) — embedding-aware chunk persistence with
//!   brute-force cosine search per ADR-0001 / ADR-0002.
//! - [`artifact_repo`] (Phase 5) — generated-artifact persistence with
//!   chained version-tracking for regenerations.
//! - [`user_repo`] (Phase 6) — user account persistence.
//! - [`project_file_repo`] (Phase 4) — project file metadata persistence.
//! - [`project_repo`] (Phase 4) — project-level persistence.
//! - [`provider_config_repo`] (Phase 4) — provider configuration persistence.
//! - [`test_run_repo`] (sandbox runner Phase 1) — sandboxed test-run,
//!   per-case, and coverage persistence.
//!
pub mod artifact_repo;
pub mod chunk_repo;
pub mod project_file_repo;
pub mod project_repo;
pub mod provider_config_repo;
pub mod test_run_repo;
pub mod user_repo;
