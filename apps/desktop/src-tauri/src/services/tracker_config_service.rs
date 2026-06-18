//! Tracker configuration service — manages encrypted API tokens.

use std::sync::Arc;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::providers::trackers::factory::build_tracker;
use crate::providers::trackers::IssueTracker;
use crate::repositories::tracker_config_repo::{self, TrackerConfigUpsert};
use crate::utils::crypto::CryptoKey;

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";
type EncryptedKeyMaterial = (Option<Vec<u8>>, Option<Vec<u8>>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfigView {
    pub id: String,
    pub tracker: String,
    pub site_url: String,
    pub email: String,
    pub has_api_token: bool,
    pub project_key: String,
    pub issue_type: String,
    pub severity_map_json: Option<String>,
    pub is_active: bool,
}

/// Save or update a tracker config. Encrypts the API token before storage.
#[allow(clippy::too_many_arguments)]
pub async fn save_config(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    tracker: String,
    site_url: String,
    email: String,
    api_token: Option<String>,
    project_key: String,
    issue_type: String,
    severity_map_json: Option<String>,
    is_active: bool,
) -> AppResult<String> {
    if tracker.trim() != "jira" {
        return Err(AppError::InvalidInput("Unsupported tracker".into()));
    }

    let existing = tracker_config_repo::fetch_for_user_tracker(
        pool,
        DEFAULT_USER_ID,
        tracker.trim(),
    )
    .await?;

    let (encrypted, nonce) = resolve_encrypted_key_material(crypto, api_token, existing.as_ref())?;
    let site_url = site_url.trim().trim_end_matches('/').to_string();

    tracker_config_repo::upsert(
        pool,
        TrackerConfigUpsert {
            tracker: tracker.trim().to_string(),
            site_url,
            email: email.trim().to_string(),
            api_token_encrypted: encrypted,
            api_token_nonce: nonce,
            project_key: project_key.trim().to_string(),
            issue_type: issue_type.trim().to_string(),
            severity_map_json,
            is_active,
        },
    )
    .await
}

/// Default page size for the configs list.
pub const DEFAULT_PAGE_LIMIT: i64 = 100;
/// Hard cap on caller-supplied page sizes.
pub const MAX_PAGE_LIMIT: i64 = 1_000;

/// List all tracker configs for the local user (tokens masked).
pub async fn list_configs(
    pool: &SqlitePool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> AppResult<Vec<TrackerConfigView>> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    let rows = tracker_config_repo::list_for_user(pool, DEFAULT_USER_ID, limit, offset).await?;
    Ok(rows
        .into_iter()
        .map(|r| TrackerConfigView {
            id: r.id,
            tracker: r.tracker,
            site_url: r.site_url,
            email: r.email,
            has_api_token: r.api_token_encrypted.is_some() && r.api_token_nonce.is_some(),
            project_key: r.project_key,
            issue_type: r.issue_type,
            severity_map_json: r.severity_map_json,
            is_active: r.is_active,
        })
        .collect())
}

/// Delete a tracker config.
pub async fn delete_config(pool: &SqlitePool, id: &str) -> AppResult<()> {
    tracker_config_repo::delete(pool, id).await
}

/// Build a live `IssueTracker` client by decrypting the stored API token.
pub fn build_tracker_client(
    crypto: &CryptoKey,
    row: &tracker_config_repo::TrackerConfigRow,
) -> AppResult<Arc<dyn IssueTracker>> {
    let api_token = match (&row.api_token_encrypted, &row.api_token_nonce) {
        (Some(ct), Some(nonce)) => crypto.decrypt_string(ct, nonce)?,
        _ => {
            return Err(AppError::Internal(anyhow::anyhow!(
                "tracker config token material is incomplete"
            )))
        }
    };

    Ok(build_tracker(&row.site_url, &row.email, &api_token))
}

/// Probe a tracker connection and return the authenticated user's display
/// name. Uses the caller-supplied API token, or falls back to the stored
/// (decrypted) token for the local user when the token is omitted.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] for an unsupported tracker type or when no
///   token is available (neither supplied nor stored).
/// - [`AppError::Tracker`] when the live connection probe fails.
pub async fn test_connection(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    tracker: &str,
    site_url: &str,
    email: &str,
    api_token: Option<String>,
) -> AppResult<String> {
    if tracker.trim() != "jira" {
        return Err(AppError::InvalidInput("Unsupported tracker type".into()));
    }

    let token = if let Some(t) = api_token {
        t
    } else {
        let existing =
            tracker_config_repo::fetch_for_user_tracker(pool, DEFAULT_USER_ID, "jira").await?;
        match existing {
            Some(row) => match (&row.api_token_encrypted, &row.api_token_nonce) {
                (Some(ct), Some(nonce)) => crypto.decrypt_string(ct, nonce)?,
                _ => return Err(AppError::InvalidInput("API token missing".into())),
            },
            None => return Err(AppError::InvalidInput("API token missing".into())),
        }
    };

    let client = build_tracker(site_url, email, &token);
    let user = client.test_connection().await?;
    Ok(user.display_name)
}

fn resolve_encrypted_key_material(
    crypto: &CryptoKey,
    api_token: Option<String>,
    existing: Option<&tracker_config_repo::TrackerConfigRow>,
) -> AppResult<EncryptedKeyMaterial> {
    match api_token {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok((None, None))
            } else {
                let (ciphertext, nonce) = crypto.encrypt(trimmed.as_bytes())?;
                Ok((Some(ciphertext), Some(nonce)))
            }
        }
        None => Ok(match existing {
            Some(row) => (row.api_token_encrypted.clone(), row.api_token_nonce.clone()),
            None => (None, None),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-tcsvc-{}.db", Uuid::new_v4()))
    }

    fn test_key() -> CryptoKey {
        CryptoKey::from_bytes([88u8; 32])
    }

    #[tokio::test]
    async fn save_and_list_tracker_configs() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let id = save_config(
            &pool,
            &crypto,
            "jira".into(),
            "https://acme.atlassian.net/".into(),
            "user@acme.com".into(),
            Some("token-123".into()),
            "PROJ".into(),
            "Task".into(),
            None,
            true,
        )
        .await
        .expect("save");

        let list = list_configs(&pool, None, None).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].tracker, "jira");
        assert_eq!(list[0].site_url, "https://acme.atlassian.net");
        assert_eq!(list[0].email, "user@acme.com");
        assert!(list[0].has_api_token);
        assert!(list[0].is_active);

        let row = tracker_config_repo::fetch(&pool, &id).await.expect("fetch");
        let client = build_tracker_client(&crypto, &row).expect("client");
        assert_eq!(client.name(), "jira");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}

