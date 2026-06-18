//! Tracker configuration and push IPC commands.

use serde::Deserialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::repositories::external_link_repo::{self, ExternalLinkRow};
use crate::services::tracker_config_service::{self, TrackerConfigView};
use crate::services::jira_push_service::{self, BulkPushResultItem, PushResult};
use crate::utils::crypto::CryptoKey;

/// IPC payload for `save_tracker_config`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTrackerArgs {
    pub tracker: String,
    pub site_url: String,
    pub email: String,
    pub api_token: Option<String>,
    pub project_key: String,
    pub issue_type: String,
    pub severity_map_json: Option<String>,
    pub is_active: bool,
}

/// IPC payload for `test_tracker_connection`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTrackerConnectionArgs {
    pub tracker: String,
    pub site_url: String,
    pub email: String,
    pub api_token: Option<String>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn save_tracker_config(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    args: SaveTrackerArgs,
) -> Result<String, String> {
    tracker_config_service::save_config(
        &pool,
        &crypto,
        args.tracker,
        args.site_url,
        args.email,
        args.api_token,
        args.project_key,
        args.issue_type,
        args.severity_map_json,
        args.is_active,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tracker_configs(
    pool: State<'_, SqlitePool>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<TrackerConfigView>, String> {
    tracker_config_service::list_configs(&pool, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn delete_tracker_config(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    tracker_config_service::delete_config(&pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn test_tracker_connection(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    args: TestTrackerConnectionArgs,
) -> Result<String, String> {
    tracker_config_service::test_connection(
        &pool,
        &crypto,
        &args.tracker,
        &args.site_url,
        &args.email,
        args.api_token,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn push_to_tracker(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    artifact_id: String,
) -> Result<PushResult, String> {
    jira_push_service::push_artifact(&pool, &crypto, &artifact_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn bulk_push_to_tracker(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    artifact_ids: Vec<String>,
) -> Result<Vec<BulkPushResultItem>, String> {
    jira_push_service::bulk_push_artifacts(&pool, &crypto, artifact_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn refresh_tracker_link_status(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    link_id: String,
) -> Result<ExternalLinkRow, String> {
    jira_push_service::refresh_link_status(&pool, &crypto, &link_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn list_external_links(
    pool: State<'_, SqlitePool>,
    artifact_id: Option<String>,
) -> Result<Vec<ExternalLinkRow>, String> {
    match artifact_id {
        Some(id) => external_link_repo::list_for_artifact(&pool, &id).await,
        None => external_link_repo::list_all(&pool).await,
    }
    .map_err(|e| e.to_string())
}
