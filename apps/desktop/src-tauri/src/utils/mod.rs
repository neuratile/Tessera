//! Pure helper functions, no side effects.
//!
//! Per `rules.md` §4.2: utilities are stateless, dependency-free, and
//! exhaustively unit-tested. If a helper needs IO or DB access, it belongs
//! in a service or repository, not here.
//!
//! Sub-modules added as needed: `secret_redaction`, `path_safety`,
//! `token_counting`.

<<<<<<< HEAD
pub mod sqlite_path;
=======
pub mod crypto;
>>>>>>> e5b6a5112e8a40bf2fe5db4140027280b536c192
pub mod telemetry;
