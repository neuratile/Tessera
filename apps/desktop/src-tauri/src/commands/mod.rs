//! Tauri IPC command handlers.
//!
//! Per `rules.md` §4.2 (adapted), this module replaces the `routes/` layer
//! prescribed for HTTP backends. Tauri commands are the IPC equivalent of
//! HTTP routes: they parse input, delegate to a service, and format the
//! response. No business logic lives here.
//!
//! Sub-modules added in Phase 6: `projects`, `analysis`, `generation`,
//! `providers`, `health`.
