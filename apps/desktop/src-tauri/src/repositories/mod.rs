//! Database access layer.
//!
//! Per `rules.md` §4.2 + §2.3: all SQL lives here. Services call
//! repositories; repositories call sqlx. Parameterized queries only —
//! never concatenate user input into SQL.
//!
//! Sub-modules added in Phase 3+: `chunk_repo`, `project_repo`,
//! `artifact_repo`, `provider_config_repo`.
