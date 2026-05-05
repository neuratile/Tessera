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
//!
pub mod artifact_repo;
pub mod chunk_repo;
<<<<<<< HEAD
pub mod user_repo;
=======
pub mod project_file_repo;
pub mod project_repo;
pub mod provider_config_repo;
>>>>>>> e5b6a5112e8a40bf2fe5db4140027280b536c192
