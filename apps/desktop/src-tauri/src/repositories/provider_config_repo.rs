//! Provider configuration repository — encrypted API key storage.
//!
//! Per `rules.md` §4.2 + §9: all SQL for `user_provider_configs` lives
//! here. API keys arrive pre-encrypted from the service layer; this
//! module stores/retrieves the ciphertext and nonce blobs without
//! touching plaintext.

use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Debug, Clone)]
pub struct ProviderConfigUpsert {
    pub provider: String,
    pub api_key_encrypted: Option<Vec<u8>>,
    pub api_key_nonce: Option<Vec<u8>>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigRow {
    pub id: String,
    pub user_id: String,
    pub provider: String,
    #[serde(skip)]
    pub api_key_encrypted: Option<Vec<u8>>,
    #[serde(skip)]
    pub api_key_nonce: Option<Vec<u8>>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Insert or update a provider config (upsert on `(user_id, provider)` unique constraint).
pub async fn upsert(pool: &SqlitePool, row: ProviderConfigUpsert) -> AppResult<String> {
    if row.provider.trim().is_empty() {
        return Err(AppError::InvalidInput("provider is empty".into()));
    }

    let now = Utc::now().to_rfc3339();
    let is_active_int: i32 = i32::from(row.is_active);

    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM user_provider_configs WHERE user_id = ? AND provider = ?")
            .bind(DEFAULT_USER_ID)
            .bind(row.provider.trim())
            .fetch_optional(pool)
            .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE user_provider_configs SET \
             api_key_encrypted = ?, api_key_nonce = ?, base_url = ?, \
             default_model = ?, is_active = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(&row.api_key_encrypted)
        .bind(&row.api_key_nonce)
        .bind(&row.base_url)
        .bind(&row.default_model)
        .bind(is_active_int)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        Ok(id)
    } else {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO user_provider_configs \
             (id, user_id, provider, api_key_encrypted, api_key_nonce, \
              base_url, default_model, is_active, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(DEFAULT_USER_ID)
        .bind(row.provider.trim())
        .bind(&row.api_key_encrypted)
        .bind(&row.api_key_nonce)
        .bind(&row.base_url)
        .bind(&row.default_model)
        .bind(is_active_int)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(id)
    }
}

pub async fn fetch(pool: &SqlitePool, id: &str) -> AppResult<ProviderConfigRow> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, provider, api_key_encrypted, api_key_nonce, \
                base_url, default_model, is_active, created_at, updated_at \
         FROM user_provider_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| AppError::NotFound(format!("provider config {id}")))
        .map(decode_row)
}

pub async fn list_for_user(pool: &SqlitePool, user_id: &str) -> AppResult<Vec<ProviderConfigRow>> {
    let rows: Vec<RawRow> = sqlx::query_as(
        "SELECT id, user_id, provider, api_key_encrypted, api_key_nonce, \
                base_url, default_model, is_active, created_at, updated_at \
         FROM user_provider_configs WHERE user_id = ? ORDER BY provider ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(decode_row).collect())
}

pub async fn fetch_active(
    pool: &SqlitePool,
    user_id: &str,
    provider: &str,
) -> AppResult<ProviderConfigRow> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, provider, api_key_encrypted, api_key_nonce, \
                base_url, default_model, is_active, created_at, updated_at \
         FROM user_provider_configs \
         WHERE user_id = ? AND provider = ? AND is_active = 1",
    )
    .bind(user_id)
    .bind(provider)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| AppError::NotFound(format!("active config for provider `{provider}`")))
        .map(decode_row)
}

pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM user_provider_configs WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("provider config {id}")));
    }
    Ok(())
}

type RawRow = (
    String,          // id
    String,          // user_id
    String,          // provider
    Option<Vec<u8>>, // api_key_encrypted
    Option<Vec<u8>>, // api_key_nonce
    Option<String>,  // base_url
    Option<String>,  // default_model
    i32,             // is_active
    String,          // created_at
    String,          // updated_at
);

fn decode_row(row: RawRow) -> ProviderConfigRow {
    let (
        id,
        user_id,
        provider,
        api_key_encrypted,
        api_key_nonce,
        base_url,
        default_model,
        is_active,
        created_at,
        updated_at,
    ) = row;
    ProviderConfigRow {
        id,
        user_id,
        provider,
        api_key_encrypted,
        api_key_nonce,
        base_url,
        default_model,
        is_active: is_active != 0,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-pc-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        (pool, path)
    }

    #[tokio::test]
    async fn upsert_insert_then_update() {
        let (pool, path) = seed_pool().await;

        let id = upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "openai".into(),
                api_key_encrypted: Some(vec![1, 2, 3]),
                api_key_nonce: Some(vec![4, 5, 6]),
                base_url: None,
                default_model: Some("gpt-4o".into()),
                is_active: true,
            },
        )
        .await
        .expect("insert");

        let fetched = fetch(&pool, &id).await.expect("fetch");
        assert_eq!(fetched.provider, "openai");
        assert!(fetched.is_active);
        assert_eq!(fetched.api_key_encrypted, Some(vec![1, 2, 3]));

        let id2 = upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "openai".into(),
                api_key_encrypted: Some(vec![7, 8, 9]),
                api_key_nonce: Some(vec![10, 11, 12]),
                base_url: Some("https://custom.example.com".into()),
                default_model: Some("gpt-4o-mini".into()),
                is_active: false,
            },
        )
        .await
        .expect("upsert");

        assert_eq!(id, id2, "upsert reuses same row");
        let updated = fetch(&pool, &id).await.expect("fetch updated");
        assert_eq!(updated.api_key_encrypted, Some(vec![7, 8, 9]));
        assert!(!updated.is_active);
        assert_eq!(
            updated.base_url.as_deref(),
            Some("https://custom.example.com")
        );

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn list_returns_configs_sorted_by_provider() {
        let (pool, path) = seed_pool().await;
        upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "openai".into(),
                api_key_encrypted: None,
                api_key_nonce: None,
                base_url: None,
                default_model: None,
                is_active: true,
            },
        )
        .await
        .expect("openai");
        upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "anthropic".into(),
                api_key_encrypted: None,
                api_key_nonce: None,
                base_url: None,
                default_model: None,
                is_active: true,
            },
        )
        .await
        .expect("anthropic");

        let list = list_for_user(&pool, DEFAULT_USER_ID).await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].provider, "anthropic");
        assert_eq!(list[1].provider, "openai");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn delete_removes_config() {
        let (pool, path) = seed_pool().await;
        let id = upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "ollama".into(),
                api_key_encrypted: None,
                api_key_nonce: None,
                base_url: None,
                default_model: None,
                is_active: true,
            },
        )
        .await
        .expect("insert");

        delete(&pool, &id).await.expect("delete");
        let err = fetch(&pool, &id).await.expect_err("must 404");
        assert_eq!(err.code(), "NOT_FOUND");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn upsert_rejects_empty_provider() {
        let (pool, path) = seed_pool().await;
        let err = upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "  ".into(),
                api_key_encrypted: None,
                api_key_nonce: None,
                base_url: None,
                default_model: None,
                is_active: false,
            },
        )
        .await
        .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
