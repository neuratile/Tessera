//! Provider configuration service — manages encrypted API keys.
//!
//! Per `rules.md` §4.2 + §9: encrypts API keys before persistence,
//! decrypts on retrieval. Never exposes plaintext keys in logs or
//! serialized responses.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::providers::factory::{ProviderConfig, ProviderKind};
use crate::repositories::provider_config_repo::{self, ProviderConfigUpsert};
use crate::utils::crypto::CryptoKey;

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

/// Frontend-safe view of a provider config. API key shown as masked
/// boolean, never plaintext.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigView {
    pub id: String,
    pub provider: String,
    pub has_api_key: bool,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub is_active: bool,
}

/// Save or update a provider config. Encrypts the API key before storage.
pub async fn save_config(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    provider: String,
    api_key: Option<String>,
    base_url: Option<String>,
    default_model: Option<String>,
    is_active: bool,
) -> AppResult<String> {
    let (encrypted, nonce) = match api_key {
        Some(ref key) if !key.trim().is_empty() => {
            let (ct, n) = crypto.encrypt(key.as_bytes())?;
            (Some(ct), Some(n))
        }
        _ => (None, None),
    };

    provider_config_repo::upsert(
        pool,
        ProviderConfigUpsert {
            provider,
            api_key_encrypted: encrypted,
            api_key_nonce: nonce,
            base_url,
            default_model,
            is_active,
        },
    )
    .await
}

/// List all provider configs for the local user (keys masked).
pub async fn list_configs(pool: &SqlitePool) -> AppResult<Vec<ProviderConfigView>> {
    let rows = provider_config_repo::list_for_user(pool, DEFAULT_USER_ID).await?;
    Ok(rows
        .into_iter()
        .map(|r| ProviderConfigView {
            id: r.id,
            provider: r.provider,
            has_api_key: r.api_key_encrypted.is_some(),
            base_url: r.base_url,
            default_model: r.default_model,
            is_active: r.is_active,
        })
        .collect())
}

/// Delete a provider config.
pub async fn delete_config(pool: &SqlitePool, id: &str) -> AppResult<()> {
    provider_config_repo::delete(pool, id).await
}

/// Build a live `ProviderConfig` by decrypting the stored API key.
/// Used by commands that need to construct an `LlmProvider` at call time.
pub fn build_provider_config(
    crypto: &CryptoKey,
    row: &provider_config_repo::ProviderConfigRow,
) -> AppResult<ProviderConfig> {
    let kind = parse_provider_kind(&row.provider)?;

    let api_key =
        match (&row.api_key_encrypted, &row.api_key_nonce) {
            (Some(ct), Some(nonce)) => {
                let plaintext = crypto.decrypt(ct, nonce)?;
                Some(String::from_utf8(plaintext).map_err(|_| {
                    AppError::Internal(anyhow::anyhow!("decrypted key is not UTF-8"))
                })?)
            }
            _ => None,
        };

    Ok(ProviderConfig {
        kind,
        base_url: row.base_url.clone(),
        api_key,
    })
}

fn parse_provider_kind(s: &str) -> AppResult<ProviderKind> {
    let json = format!("\"{s}\"");
    serde_json::from_str::<ProviderKind>(&json)
        .map_err(|_| AppError::InvalidInput(format!("unknown provider kind `{s}`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-pcsvc-{}.db", Uuid::new_v4()))
    }

    fn test_key() -> CryptoKey {
        CryptoKey::from_bytes([99u8; 32])
    }

    #[tokio::test]
    async fn save_and_list_round_trips() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        save_config(
            &pool,
            &crypto,
            "openai".into(),
            Some("sk-test-123".into()),
            None,
            Some("gpt-4o".into()),
            true,
        )
        .await
        .expect("save");

        let list = list_configs(&pool).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].provider, "openai");
        assert!(list[0].has_api_key);
        assert!(list[0].is_active);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn build_provider_config_decrypts_key() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let id = save_config(
            &pool,
            &crypto,
            "openai".into(),
            Some("sk-secret".into()),
            None,
            None,
            true,
        )
        .await
        .expect("save");

        let row = provider_config_repo::fetch(&pool, &id)
            .await
            .expect("fetch");
        let config = build_provider_config(&crypto, &row).expect("build");
        assert_eq!(config.kind, ProviderKind::OpenAi);
        assert_eq!(config.api_key.as_deref(), Some("sk-secret"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parse_provider_kind_covers_all_variants() {
        let cases = [
            ("ollama", ProviderKind::Ollama),
            ("openai", ProviderKind::OpenAi),
            ("anthropic", ProviderKind::Anthropic),
            ("openrouter", ProviderKind::OpenRouter),
            ("ollama-cloud", ProviderKind::OllamaCloud),
        ];
        for (s, expected) in cases {
            assert_eq!(parse_provider_kind(s).expect(s), expected);
        }
    }

    #[test]
    fn parse_provider_kind_rejects_unknown() {
        let err = parse_provider_kind("unknown-provider").expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn view_never_leaks_key() {
        let view = ProviderConfigView {
            id: "x".into(),
            provider: "openai".into(),
            has_api_key: true,
            base_url: None,
            default_model: None,
            is_active: true,
        };
        let json = serde_json::to_string(&view).expect("serialize");
        assert!(!json.contains("sk-"));
        assert!(json.contains("hasApiKey"));
    }
}
