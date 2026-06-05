//! Issue entity (the central work item).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A work item (epic, story, task, bug, subtask).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: Uuid,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub sprint_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub issue_key: String,
    pub issue_type: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub assignee_id: Option<Uuid>,
    pub reporter_id: Uuid,
    pub story_points: Option<i32>,
    pub due_date: Option<DateTime<Utc>>,
    pub git_branch: Option<String>,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Payload for creating an issue.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIssue {
    pub issue_type: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    pub assignee_id: Option<Uuid>,
    pub column_id: Option<Uuid>,
    pub sprint_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub story_points: Option<i32>,
    pub due_date: Option<DateTime<Utc>>,
    pub git_branch: Option<String>,
}

fn default_priority() -> String {
    "medium".to_string()
}

use crate::models::double_option;

/// Payload for updating an issue.
///
/// Nullable columns use `Option<Option<T>>` so a client can send an explicit
/// `null` to clear the field, while omitting it leaves the value unchanged.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIssue {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub issue_type: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub assignee_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub sprint_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub story_points: Option<Option<i32>>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_date: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub git_branch: Option<Option<String>>,
}

/// Payload for moving an issue to a different column / position.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveIssue {
    pub column_id: Uuid,
    pub position: i32,
}

