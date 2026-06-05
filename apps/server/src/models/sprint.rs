//! Sprint entity (time-boxed iteration for scrum boards).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A sprint on a scrum board.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Sprint {
    pub id: Uuid,
    pub board_id: Uuid,
    pub name: String,
    pub goal: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Payload for creating a sprint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSprint {
    pub name: String,
    pub goal: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

/// Payload for updating a sprint.
///
/// Nullable fields use `Option<Option<T>>`: an explicit `null` clears the
/// value, omission leaves it unchanged (see `models::double_option`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSprint {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "crate::models::double_option")]
    pub goal: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::models::double_option")]
    pub start_date: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "crate::models::double_option")]
    pub end_date: Option<Option<DateTime<Utc>>>,
}

