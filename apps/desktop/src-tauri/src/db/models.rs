//! Database row models for core tables.
//!
//! These structs mirror the persisted `SQLite` schema and are used by
//! repositories / commands when decoding query results.

use serde::Serialize;
use sqlx::FromRow;

/// Row model for `users`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub plan: String,
    pub password_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Row model for `projects`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectRow {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub root_path: String,
    pub path: Option<String>,
    pub file_count: i64,
    pub total_size_bytes: i64,
    pub status: String,
    pub language_breakdown: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Row model for `artifacts`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ArtifactRow {
    pub id: String,
    pub project_id: String,
    pub artifact_type: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub title: String,
    pub content_md: String,
    pub content: Option<String>,
    pub structured_data: String,
    pub status: String,
    pub version: i64,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Row model for `code_chunks`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CodeChunkRow {
    pub id: String,
    pub project_id: String,
    pub file_id: String,
    pub file_path: Option<String>,
    pub chunk_type: String,
    pub name: Option<String>,
    pub content: String,
    pub start_line: i64,
    pub end_line: i64,
    pub token_count: i64,
    pub embedding: Option<Vec<u8>>,
    pub embedding_dim: Option<i64>,
    pub embedding_provider: Option<String>,
    pub embedding_model: Option<String>,
    pub metadata: String,
    pub created_at: String,
    pub updated_at: String,
}
