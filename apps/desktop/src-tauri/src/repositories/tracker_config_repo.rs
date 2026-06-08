//! Tracker configuration repository — encrypted API token storage.

use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Debug, Clone)]
pub struct TrackerConfigUpsert {
    pub tracker: String,
    pub site_url: String,
    pub email: String,
    pub api_token_encrypted: Option<Vec<u8>>,
    pub api_token_nonce: Option<Vec<u8>>,
    pub project_key: String,
    pub issue_type: String,
    pub severity_map_json: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfigRow {
    pub id: String,
    pub user_id: String,
    pub tracker: String,
    pub site_url: String,
    pub email: String,
    #[serde(skip)]
    pub api_token_encrypted: Option<Vec<u8>>,
    #[serde(skip)]
    pub api_token_nonce: Option<Vec<u8>>,
    pub project_key: String,
    pub issue_type: String,
    pub severity_map_json: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Insert or update a tracker config (upsert on `(user_id, tracker)` unique constraint).
pub async fn upsert(pool: &SqlitePool, row: TrackerConfigUpsert) -> AppResult<String> {
    if row.tracker.trim().is_empty() {
        return Err(AppError::InvalidInput("tracker is empty".into()));
    }

    let now = Utc::now().to_rfc3339();
    let is_active_int: i32 = i32::from(row.is_active);

    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM tracker_configs WHERE user_id = ? AND tracker = ?")
            .bind(DEFAULT_USER_ID)
            .bind(row.tracker.trim())
            .fetch_optional(pool)
            .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE tracker_configs SET \
             site_url = ?, email = ?, api_token_encrypted = ?, api_token_nonce = ?, \
             project_key = ?, issue_type = ?, severity_map_json = ?, is_active = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(&row.site_url)
        .bind(&row.email)
        .bind(&row.api_token_encrypted)
        .bind(&row.api_token_nonce)
        .bind(&row.project_key)
        .bind(&row.issue_type)
        .bind(&row.severity_map_json)
        .bind(is_active_int)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        Ok(id)
    } else {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO tracker_configs \
             (id, user_id, tracker, site_url, email, api_token_encrypted, api_token_nonce, \
              project_key, issue_type, severity_map_json, is_active, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(DEFAULT_USER_ID)
        .bind(row.tracker.trim())
        .bind(&row.site_url)
        .bind(&row.email)
        .bind(&row.api_token_encrypted)
        .bind(&row.api_token_nonce)
        .bind(&row.project_key)
        .bind(&row.issue_type)
        .bind(&row.severity_map_json)
        .bind(is_active_int)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(id)
    }
}

pub async fn fetch(pool: &SqlitePool, id: &str) -> AppResult<TrackerConfigRow> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, tracker, site_url, email, api_token_encrypted, api_token_nonce, \
                project_key, issue_type, severity_map_json, is_active, created_at, updated_at \
         FROM tracker_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| AppError::NotFound(format!("tracker config {id}")))
        .map(decode_row)
}

/// Fetch the config row for one `(user_id, tracker)` pair, if present.
pub async fn fetch_for_user_tracker(
    pool: &SqlitePool,
    user_id: &str,
    tracker: &str,
) -> AppResult<Option<TrackerConfigRow>> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, tracker, site_url, email, api_token_encrypted, api_token_nonce, \
                project_key, issue_type, severity_map_json, is_active, created_at, updated_at \
         FROM tracker_configs \
         WHERE user_id = ? AND tracker = ?",
    )
    .bind(user_id)
    .bind(tracker)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(decode_row))
}

pub async fn list_for_user(
    pool: &SqlitePool,
    user_id: &str,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<TrackerConfigRow>> {
    let rows: Vec<RawRow> = sqlx::query_as(
        "SELECT id, user_id, tracker, site_url, email, api_token_encrypted, api_token_nonce, \
                project_key, issue_type, severity_map_json, is_active, created_at, updated_at \
         FROM tracker_configs WHERE user_id = ? ORDER BY tracker ASC \
         LIMIT ? OFFSET ?",
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(decode_row).collect())
}

pub async fn fetch_active(
    pool: &SqlitePool,
    user_id: &str,
    tracker: &str,
) -> AppResult<TrackerConfigRow> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, tracker, site_url, email, api_token_encrypted, api_token_nonce, \
                project_key, issue_type, severity_map_json, is_active, created_at, updated_at \
         FROM tracker_configs \
         WHERE user_id = ? AND tracker = ? AND is_active = 1",
    )
    .bind(user_id)
    .bind(tracker)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| AppError::NotFound(format!("active config for tracker `{tracker}`")))
        .map(decode_row)
}

pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM tracker_configs WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("tracker config {id}")));
    }
    Ok(())
}

type RawRow = (
    String,          // id
    String,          // user_id
    String,          // tracker
    String,          // site_url
    String,          // email
    Option<Vec<u8>>, // api_token_encrypted
    Option<Vec<u8>>, // api_token_nonce
    String,          // project_key
    String,          // issue_type
    Option<String>,  // severity_map_json
    i32,             // is_active
    String,          // created_at
    String,          // updated_at
);

fn decode_row(row: RawRow) -> TrackerConfigRow {
    let (
        id,
        user_id,
        tracker,
        site_url,
        email,
        api_token_encrypted,
        api_token_nonce,
        project_key,
        issue_type,
        severity_map_json,
        is_active,
        created_at,
        updated_at,
    ) = row;
    TrackerConfigRow {
        id,
        user_id,
        tracker,
        site_url,
        email,
        api_token_encrypted,
        api_token_nonce,
        project_key,
        issue_type,
        severity_map_json,
        is_active: is_active != 0,
        created_at,
        updated_at,
    }
}
