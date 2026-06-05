//! Domain model structs for all database entities.

/// Deserializes a field that distinguishes "absent" from "explicitly null".
/// Absent -> outer `None` (leave unchanged); `null` -> `Some(None)` (clear);
/// value -> `Some(Some(v))` (set). Use with
/// `#[serde(default, deserialize_with = "crate::models::double_option")]`.
pub fn double_option<'de, T, D>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

pub mod activity;
pub mod board;
pub mod comment;
pub mod issue;
pub mod label;
pub mod sprint;
pub mod team;
pub mod user;

pub use activity::{ActivityLog, CreateActivity};
pub use board::{Board, BoardColumn, CreateBoard, CreateColumn};
pub use comment::{Comment, CreateComment};
pub use issue::{CreateIssue, Issue, MoveIssue, UpdateIssue};
pub use label::{CreateLabel, Label};
pub use sprint::{CreateSprint, Sprint};
pub use team::{CreateTeam, Team, TeamMember};
pub use user::{CreateUser, User};
