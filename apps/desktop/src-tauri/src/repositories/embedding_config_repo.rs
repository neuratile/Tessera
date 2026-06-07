//! Embedding configuration repository — encrypted API key storage.
//!
//! Per `rules.md` §4.2 + §9: all SQL for `user_embedding_configs`
//! lives here. API keys arrive pre-encrypted from the service layer;
//! this module stores/retrieves ciphertext and nonce blobs without
//! touching plaintext. Mirrors `provider_config_repo` shape, plus the
//! embedding-specific `model` / `dimension` columns
//! (`plan/EMBEDDING_PROVIDER_SELECT.md` §5.1).

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct EmbeddingConfigUpsert {
    pub provider: String,
    pub model: String,
    pub dimension: u32,
    pub base_url: Option<String>,
    pub api_key_encrypted: Option<Vec<u8>>,
    pub api_key_nonce: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfigRow {
    pub id: String,
    pub user_id: String,
    pub provider: String,
    pub model: String,
    pub dimension: u32,
    pub base_url: Option<String>,
    pub api_key_encrypted: Option<Vec<u8>>,
    pub api_key_nonce: Option<Vec<u8>>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Upsert one `(user_id, provider)` row and mark it the single active
/// embedding selection for the user — every other row flips to
/// `is_active = 0` inside the same transaction so exactly one row per
/// user carries the flag.
pub async fn upsert_active(
    pool: &SqlitePool,
    user_id: &str,
    row: EmbeddingConfigUpsert,
) -> AppResult<String> {
    if row.provider.trim().is_empty() {
        return Err(AppError::InvalidInput("provider is empty".into()));
    }
    if row.model.trim().is_empty() {
        return Err(AppError::InvalidInput("model is empty".into()));
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE user_embedding_configs SET is_active = 0, updated_at = ? WHERE user_id = ?")
        .bind(&now)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM user_embedding_configs WHERE user_id = ? AND provider = ?")
            .bind(user_id)
            .bind(row.provider.trim())
            .fetch_optional(&mut *tx)
            .await?;

    let id = if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE user_embedding_configs SET \
             model = ?, dimension = ?, base_url = ?, \
             api_key_encrypted = ?, api_key_nonce = ?, \
             is_active = 1, updated_at = ? \
             WHERE id = ?",
        )
        .bind(row.model.trim())
        .bind(i64::from(row.dimension))
        .bind(&row.base_url)
        .bind(&row.api_key_encrypted)
        .bind(&row.api_key_nonce)
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        id
    } else {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO user_embedding_configs \
             (id, user_id, provider, model, dimension, base_url, \
              api_key_encrypted, api_key_nonce, is_active, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(row.provider.trim())
        .bind(row.model.trim())
        .bind(i64::from(row.dimension))
        .bind(&row.base_url)
        .bind(&row.api_key_encrypted)
        .bind(&row.api_key_nonce)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        id
    };

    tx.commit().await?;
    Ok(id)
}

/// Fetch the single active embedding config for a user, if any.
pub async fn fetch_active(pool: &SqlitePool, user_id: &str) -> AppResult<Option<EmbeddingConfigRow>> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, provider, model, dimension, base_url, \
                api_key_encrypted, api_key_nonce, is_active, created_at, updated_at \
         FROM user_embedding_configs \
         WHERE user_id = ? AND is_active = 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    row.map(decode_row).transpose()
}

/// Fetch the config row for one `(user_id, provider)` pair, if present.
/// Used to surface a previously-stored key/model when the user switches
/// back to a provider in the Settings UI.
pub async fn fetch_for_user_provider(
    pool: &SqlitePool,
    user_id: &str,
    provider: &str,
) -> AppResult<Option<EmbeddingConfigRow>> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, user_id, provider, model, dimension, base_url, \
                api_key_encrypted, api_key_nonce, is_active, created_at, updated_at \
         FROM user_embedding_configs \
         WHERE user_id = ? AND provider = ?",
    )
    .bind(user_id)
    .bind(provider)
    .fetch_optional(pool)
    .await?;

    row.map(decode_row).transpose()
}

type RawRow = (
    String,          // id
    String,          // user_id
    String,          // provider
    String,          // model
    i64,             // dimension
    Option<String>,  // base_url
    Option<Vec<u8>>, // api_key_encrypted
    Option<Vec<u8>>, // api_key_nonce
    i32,             // is_active
    String,          // created_at
    String,          // updated_at
);

fn decode_row(row: RawRow) -> AppResult<EmbeddingConfigRow> {
    let (
        id,
        user_id,
        provider,
        model,
        dimension,
        base_url,
        api_key_encrypted,
        api_key_nonce,
        is_active,
        created_at,
        updated_at,
    ) = row;
    let dimension = u32::try_from(dimension).map_err(|_| {
        AppError::Database(sqlx::Error::Decode(
            format!("embedding config {id} has out-of-range dimension {dimension}").into(),
        ))
    })?;
    Ok(EmbeddingConfigRow {
        id,
        user_id,
        provider,
        model,
        dimension,
        base_url,
        api_key_encrypted,
        api_key_nonce,
        is_active: is_active != 0,
        created_at,
        updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;

    const USER: &str = "00000000-0000-4000-8000-000000000001";

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-emb-{}.db", Uuid::new_v4()))
    }

    fn ollama_upsert() -> EmbeddingConfigUpsert {
        EmbeddingConfigUpsert {
            provider: "ollama".into(),
            model: "nomic-embed-text".into(),
            dimension: 768,
            base_url: None,
            api_key_encrypted: None,
            api_key_nonce: None,
        }
    }

    #[tokio::test]
    async fn upsert_insert_then_update_reuses_row() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");

        let id = upsert_active(&pool, USER, ollama_upsert()).await.expect("insert");

        let mut updated = ollama_upsert();
        updated.model = "mxbai-embed-large".into();
        updated.dimension = 1024;
        let id2 = upsert_active(&pool, USER, updated).await.expect("update");

        assert_eq!(id, id2, "upsert reuses same row");
        let active = fetch_active(&pool, USER).await.expect("fetch").expect("row");
        assert_eq!(active.model, "mxbai-embed-large");
        assert_eq!(active.dimension, 1024);
        assert!(active.is_active);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn upsert_active_flips_previous_active_off() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");

        upsert_active(&pool, USER, ollama_upsert()).await.expect("ollama");
        upsert_active(
            &pool,
            USER,
            EmbeddingConfigUpsert {
                provider: "openai".into(),
                model: "text-embedding-3-small".into(),
                dimension: 1536,
                base_url: None,
                api_key_encrypted: Some(vec![1, 2, 3]),
                api_key_nonce: Some(vec![4, 5, 6]),
            },
        )
        .await
        .expect("openai");

        let active = fetch_active(&pool, USER).await.expect("fetch").expect("row");
        assert_eq!(active.provider, "openai");

        // Previous row still exists (key/model preserved) but inactive.
        let ollama = fetch_for_user_provider(&pool, USER, "ollama")
            .await
            .expect("fetch")
            .expect("row");
        assert!(!ollama.is_active);
        assert_eq!(ollama.model, "nomic-embed-text");

        let actives: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM user_embedding_configs WHERE user_id = ? AND is_active = 1",
        )
        .bind(USER)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(actives.0, 1, "exactly one active row");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn fetch_active_returns_none_when_unset() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let active = fetch_active(&pool, USER).await.expect("fetch");
        assert!(active.is_none());
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn upsert_rejects_empty_provider_and_model() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");

        let mut bad = ollama_upsert();
        bad.provider = "  ".into();
        let err = upsert_active(&pool, USER, bad).await.expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");

        let mut bad = ollama_upsert();
        bad.model = String::new();
        let err = upsert_active(&pool, USER, bad).await.expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
