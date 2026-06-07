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
use crate::utils::provider_base_url::{
    normalize_gemini_base_url, normalize_ollama_base_url, normalize_openai_compatible_base_url,
};

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";
type EncryptedKeyMaterial = (Option<Vec<u8>>, Option<Vec<u8>>);

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
    let provider_kind = ProviderKind::from_str_value(&provider)?;
    let existing = provider_config_repo::fetch_for_user_provider(
        pool,
        DEFAULT_USER_ID,
        provider_kind.as_str(),
    )
    .await?;
    let (encrypted, nonce) = resolve_encrypted_key_material(crypto, api_key, existing.as_ref())?;
    let base_url = resolve_base_url(
        provider_kind,
        base_url,
        existing.as_ref().and_then(|row| row.base_url.clone()),
    );
    let default_model = resolve_optional_string(
        default_model,
        existing.as_ref().and_then(|row| row.default_model.clone()),
    );

    provider_config_repo::upsert(
        pool,
        ProviderConfigUpsert {
            provider: provider_kind.as_str().to_string(),
            api_key_encrypted: encrypted,
            api_key_nonce: nonce,
            base_url,
            default_model,
            is_active,
        },
    )
    .await
}

/// Default page size for the configs list. Provider configs are
/// few-per-user in practice (one per LLM provider) but the cap keeps
/// the IPC payload bounded.
pub const DEFAULT_PAGE_LIMIT: i64 = 100;
/// Hard cap on caller-supplied page sizes.
pub const MAX_PAGE_LIMIT: i64 = 1_000;

/// List all provider configs for the local user (keys masked).
pub async fn list_configs(
    pool: &SqlitePool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> AppResult<Vec<ProviderConfigView>> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    let rows =
        provider_config_repo::list_for_user(pool, DEFAULT_USER_ID, limit, offset).await?;
    Ok(rows
        .into_iter()
        .map(|r| ProviderConfigView {
            id: r.id,
            provider: r.provider,
            has_api_key: r.api_key_encrypted.is_some() && r.api_key_nonce.is_some(),
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
    let kind = ProviderKind::from_str_value(&row.provider)?;

    let api_key = match (&row.api_key_encrypted, &row.api_key_nonce) {
        (Some(ct), Some(nonce)) => Some(crypto.decrypt_string(ct, nonce)?),
        (None, None) => None,
        _ => {
            return Err(AppError::Internal(anyhow::anyhow!(
                "provider config key material is incomplete"
            )))
        }
    };

    Ok(ProviderConfig {
        kind,
        base_url: row.base_url.clone(),
        api_key,
    })
}

fn resolve_encrypted_key_material(
    crypto: &CryptoKey,
    api_key: Option<String>,
    existing: Option<&provider_config_repo::ProviderConfigRow>,
) -> AppResult<EncryptedKeyMaterial> {
    match api_key {
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
            Some(row) => (row.api_key_encrypted.clone(), row.api_key_nonce.clone()),
            None => (None, None),
        }),
    }
}

fn resolve_optional_string(value: Option<String>, existing: Option<String>) -> Option<String> {
    match value {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => existing,
    }
}

fn resolve_base_url(
    kind: ProviderKind,
    value: Option<String>,
    existing: Option<String>,
) -> Option<String> {
    resolve_optional_string(value, existing).map(|raw| normalize_base_url(kind, &raw))
}

/// Normalize a provider base URL according to its `kind`: Ollama hosts
/// strip `/api` and `/v1` suffixes, OpenAI-compatible hosts strip `/v1`,
/// Gemini hosts strip the `/v1beta/openai` compatibility path.
///
/// Shared by `provider_connection_service` so the normalization rule has a
/// single definition across config persistence and connection testing.
pub(crate) fn normalize_base_url(kind: ProviderKind, raw: &str) -> String {
    match kind {
        ProviderKind::Ollama | ProviderKind::OllamaCloud => normalize_ollama_base_url(raw),
        ProviderKind::OpenAi | ProviderKind::OpenRouter | ProviderKind::Anthropic => {
            normalize_openai_compatible_base_url(raw)
        }
        ProviderKind::Gemini => normalize_gemini_base_url(raw),
    }
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

        let list = list_configs(&pool, None, None).await.expect("list");
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
            ("gemini", ProviderKind::Gemini),
        ];
        for (s, expected) in cases {
            assert_eq!(ProviderKind::from_str_value(s).expect(s), expected);
        }
    }

    #[test]
    fn parse_provider_kind_rejects_unknown() {
        let err = ProviderKind::from_str_value("unknown-provider").expect_err("must reject");
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

    #[tokio::test]
    async fn save_config_preserves_existing_key_when_api_key_omitted() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let id = save_config(
            &pool,
            &crypto,
            "openai".into(),
            Some("sk-original".into()),
            None,
            Some("gpt-4o".into()),
            true,
        )
        .await
        .expect("initial save");

        save_config(
            &pool,
            &crypto,
            "openai".into(),
            None,
            Some("https://example.test/v1".into()),
            Some("gpt-4o-mini".into()),
            true,
        )
        .await
        .expect("update without key");

        let row = provider_config_repo::fetch(&pool, &id)
            .await
            .expect("fetch");
        let config = build_provider_config(&crypto, &row).expect("build");

        assert_eq!(config.api_key.as_deref(), Some("sk-original"));
        assert_eq!(row.base_url.as_deref(), Some("https://example.test"));
        assert_eq!(row.default_model.as_deref(), Some("gpt-4o-mini"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn save_config_rejects_unknown_provider() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let err = save_config(
            &pool,
            &crypto,
            "made-up-provider".into(),
            None,
            None,
            None,
            true,
        )
        .await
        .expect_err("must reject");

        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_provider_config_rejects_partial_key_material() {
        let crypto = test_key();
        let row = provider_config_repo::ProviderConfigRow {
            id: "cfg".into(),
            user_id: DEFAULT_USER_ID.into(),
            provider: "openai".into(),
            api_key_encrypted: Some(vec![1, 2, 3]),
            api_key_nonce: None,
            base_url: None,
            default_model: None,
            is_active: true,
            created_at: "2026-05-06T00:00:00.000Z".into(),
            updated_at: "2026-05-06T00:00:00.000Z".into(),
        };

        let err = build_provider_config(&crypto, &row).expect_err("must reject");
        assert_eq!(err.code(), "INTERNAL_ERROR");
    }

    #[test]
    fn normalize_base_url_strips_provider_specific_suffixes() {
        assert_eq!(
            normalize_base_url(ProviderKind::Ollama, "http://localhost:11434/api/"),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_base_url(ProviderKind::Ollama, "http://localhost:11434/v1/"),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_base_url(ProviderKind::OpenAi, "https://api.openai.com/v1/"),
            "https://api.openai.com"
        );
        assert_eq!(
            normalize_base_url(
                ProviderKind::Gemini,
                "https://generativelanguage.googleapis.com/v1beta/openai/"
            ),
            "https://generativelanguage.googleapis.com"
        );
    }
}
