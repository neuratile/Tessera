//! Pure helper functions, no side effects.
//!
//! Per `rules.md` §4.2: utilities are stateless, dependency-free, and
//! exhaustively unit-tested. If a helper needs IO or DB access, it belongs
//! in a service or repository, not here.
//!
//! Sub-modules added as needed: `secret_redaction`, `path_safety`,
//! `token_counting`.

pub mod crypto;
pub mod telemetry;
