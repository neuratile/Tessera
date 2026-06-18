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
//! - [`embedding_config_repo`] — embedding provider selection persistence
//!   (`plan/versions/v1/EMBEDDING_PROVIDER_SELECT.md`).
//! - [`test_run_repo`] (sandbox runner Phase 1) — sandboxed test-run,
//!   per-case, and coverage persistence.
//! - [`flaky_check_repo`] — persisted flaky-check history (header + per-test
//!   verdicts) for the flaky-test trend UI.
//! - [`mutation_check_repo`] — persisted mutation-score history (header +
//!   per-mutant verdicts) for the mutation-score trend UI.
//! - [`test_case_result_repo`] (Test Case table) — per-case
//!   execution-outcome sidecar (Actual output / Result + remarks).
//! - [`health_repo`] — database liveness probe for the health check.
//!
pub mod artifact_repo;
pub mod chunk_repo;
pub mod embedding_config_repo;
pub mod external_link_repo;
pub mod flaky_check_repo;
pub mod health_repo;
pub mod mutation_check_repo;
pub mod project_file_repo;
pub mod project_repo;
pub mod provider_config_repo;
pub mod test_case_result_repo;
pub mod test_run_repo;
pub mod tracker_config_repo;
pub mod user_repo;

