//! Board and board-column entities.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A project board (kanban or scrum).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub key: String,
    pub description: Option<String>,
    pub board_type: String,
    pub issue_counter: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A column on a board.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BoardColumn {
    pub id: Uuid,
    pub board_id: Uuid,
    pub name: String,
    pub color: String,
    pub position: i32,
    pub wip_limit: Option<i32>,
    /// Marks the column whose issues count as completed for sprint completion.
    pub is_done: bool,
}

/// Payload for creating a board.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoard {
    pub name: String,
    pub key: String,
    pub description: Option<String>,
    #[serde(default = "default_board_type")]
    pub board_type: String,
}

fn default_board_type() -> String {
    "kanban".to_string()
}

/// Payload for creating a column.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateColumn {
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    pub position: i32,
    pub wip_limit: Option<i32>,
}

fn default_color() -> String {
    "#6b7280".to_string()
}

/// Payload for updating a board.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoard {
    pub name: Option<String>,
    pub description: Option<String>,
    pub board_type: Option<String>,
}

