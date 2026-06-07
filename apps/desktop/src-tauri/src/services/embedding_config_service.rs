//! Embedding configuration service — selection, key handling, and
//! provider resolution (`plan/EMBEDDING_PROVIDER_SELECT.md`).
//!
//! Per `rules.md` §4.2 + §9: encrypts API keys before persistence,
//! decrypts only into in-memory [`EmbeddingConfig`] values, and never
//! exposes plaintext over IPC. This module is the single production
//! entry point for building an [`EmbeddingProvider`] — commands must
//! call [`resolve_provider`] instead of constructing providers
//! directly, so the user's embedding selection applies everywhere
//! (analysis and generation alike).

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::providers::embeddings::ollama::{
    DEFAULT_DIMENSION as OLLAMA_DEFAULT_DIMENSION, DEFAULT_MODEL as OLLAMA_DEFAULT_MODEL,
};
use crate::providers::embeddings::{chunk_scope_string, EmbeddingProvider};
use crate::providers::factory::{self, EmbeddingConfig, EmbeddingProviderKind};
use crate::repositories::embedding_config_repo::{self, EmbeddingConfigUpsert};
use crate::repositories::{chunk_repo, provider_config_repo};
use crate::utils::crypto::CryptoKey;
use crate::utils::provider_base_url::{
    normalize_gemini_base_url, normalize_ollama_base_url, normalize_openai_compatible_base_url,
};

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

/// Upper bound on accepted embedding dimensions. Generous headroom over
/// today's largest cloud models (3072) without letting a typo allocate
/// absurd vectors.
pub const MAX_DIMENSION: u32 = 8_192;

/// Input string used by the connection-test probe. Content is
/// irrelevant — only the returned vector's length matters.
const PROBE_INPUT: &str = "tessera embedding dimension probe";

/// Frontend-safe view of the embedding config. API key shown as masked
/// boolean, never plaintext. `id` is `None` when the user has no stored
/// row yet and the implicit local-Ollama default applies.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfigView {
    pub id: Option<String>,
    pub provider: String,
    pub model: String,
    pub dimension: u32,
    pub base_url: Option<String>,
    pub has_api_key: bool,
    pub is_active: bool,
}

/// Arguments accepted by [`save_config`] and [`test_connection`].
#[derive(Debug, Clone)]
pub struct SaveEmbeddingConfigArgs {
    pub provider: String,
    pub model: String,
    pub dimension: u32,
    pub base_url: Option<String>,
    /// `Some(non-empty)` stores a new key, `Some(empty)` clears the
    /// stored key, `None` preserves whatever is stored (same contract
    /// as `provider_config_service`).
    pub api_key: Option<String>,
}

/// Result of the Settings connection test.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEmbeddingResult {
    pub latency_ms: u64,
    pub detected_dimension: u32,
}

/// What one project's chunk index was embedded with, versus the active
/// config. `indexed_with` is `None` for never-indexed projects.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub project_id: String,
    pub embedded_chunks: u64,
    pub indexed_with: Option<EmbeddingSignatureView>,
    pub active_config: EmbeddingSignatureView,
    pub is_stale: bool,
}

/// `(provider, model, dimension)` triple identifying one embedding
/// space. `provider` for indexed chunks is the raw stored string —
/// legacy rows may carry identifiers from removed providers, so it is
/// not parsed back into [`EmbeddingProviderKind`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSignatureView {
    pub provider: String,
    pub model: String,
    pub dimension: u32,
}

/// The active embedding config as a frontend view, falling back to the
/// implicit local-Ollama default when nothing is stored.
pub async fn get_active_view(pool: &SqlitePool) -> AppResult<EmbeddingConfigView> {
    match embedding_config_repo::fetch_active(pool, DEFAULT_USER_ID).await? {
        Some(row) => Ok(EmbeddingConfigView {
            id: Some(row.id),
            provider: row.provider,
            model: row.model,
            dimension: row.dimension,
            base_url: row.base_url,
            has_api_key: row.api_key_encrypted.is_some() && row.api_key_nonce.is_some(),
            is_active: row.is_active,
        }),
        None => Ok(default_view()),
    }
}

/// Validate + persist the embedding selection and mark it active.
pub async fn save_config(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    args: SaveEmbeddingConfigArgs,
) -> AppResult<EmbeddingConfigView> {
    let kind = EmbeddingProviderKind::from_str_value(&args.provider)?;
    let model = validate_model(kind, &args.model)?;
    validate_dimension(args.dimension)?;

    let existing =
        embedding_config_repo::fetch_for_user_provider(pool, DEFAULT_USER_ID, kind.as_str())
            .await?;
    let (encrypted, nonce) = match args.api_key {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                (None, None)
            } else {
                let (ciphertext, nonce) = crypto.encrypt(trimmed.as_bytes())?;
                (Some(ciphertext), Some(nonce))
            }
        }
        None => existing.as_ref().map_or((None, None), |row| {
            (row.api_key_encrypted.clone(), row.api_key_nonce.clone())
        }),
    };
    let base_url = args
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|raw| normalize_base_url(kind, raw));

    let id = embedding_config_repo::upsert_active(
        pool,
        DEFAULT_USER_ID,
        EmbeddingConfigUpsert {
            provider: kind.as_str().to_string(),
            model: model.clone(),
            dimension: args.dimension,
            base_url: base_url.clone(),
            api_key_encrypted: encrypted.clone(),
            api_key_nonce: nonce,
        },
    )
    .await?;

    Ok(EmbeddingConfigView {
        id: Some(id),
        provider: kind.as_str().to_string(),
        model,
        dimension: args.dimension,
        base_url,
        has_api_key: encrypted.is_some(),
        is_active: true,
    })
}

/// Resolve the active embedding selection into a live provider. This
/// is the only production path that constructs an `EmbeddingProvider`.
///
/// `ollama_base_url` is the app-config fallback used when no row is
/// stored (implicit default) or when a local-Ollama row omits its base
/// URL.
pub async fn resolve_provider(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    ollama_base_url: &str,
) -> AppResult<Arc<dyn EmbeddingProvider>> {
    let config = resolve_config(pool, crypto, ollama_base_url).await?;
    if config.dimension == 0 {
        // Defense in depth: probe-mode providers must never reach the
        // analysis/generation pipelines (save_config validates >= 1).
        return Err(AppError::InvalidInput(
            "embedding config has dimension 0".into(),
        ));
    }
    Ok(factory::build_embedding_provider(&config)?)
}

/// Probe the given (possibly unsaved) settings: embed one string and
/// report latency plus the model's native dimension, so the UI can
/// auto-fill the dimension field before saving.
pub async fn test_connection(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    ollama_base_url: &str,
    args: SaveEmbeddingConfigArgs,
) -> AppResult<TestEmbeddingResult> {
    let kind = EmbeddingProviderKind::from_str_value(&args.provider)?;
    let model = validate_model(kind, &args.model)?;

    let api_key = match args.api_key.as_deref().map(str::trim) {
        Some(key) if !key.is_empty() => Some(key.to_string()),
        // Empty-or-omitted test key falls back to stored material so
        // "Test" works on an already-saved config without re-entry.
        _ => resolve_api_key(pool, crypto, kind).await?,
    };
    let base_url = args
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|raw| normalize_base_url(kind, raw))
        .or_else(|| ollama_fallback_base(kind, ollama_base_url));

    let config = EmbeddingConfig {
        kind,
        model,
        dimension: 0, // probe mode — discover the dimension
        base_url,
        api_key,
    };
    let provider = factory::build_embedding_provider(&config)?;

    let started = Instant::now();
    let vectors = provider
        .embed(vec![PROBE_INPUT.to_string()])
        .await
        .map_err(AppError::Llm)?;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let detected = vectors
        .first()
        .map(Vec::len)
        .and_then(|len| u32::try_from(len).ok())
        .filter(|len| *len > 0)
        .ok_or_else(|| {
            AppError::InvalidInput("embedding probe returned no vector".into())
        })?;

    Ok(TestEmbeddingResult {
        latency_ms,
        detected_dimension: detected,
    })
}

/// Compare what a project's chunks were embedded with against the
/// active config. Stale = at least one embedded chunk exists and any
/// stored signature differs from the active one — RAG retrieval is
/// scoped to the active signature, so stale chunks silently stop
/// matching until the user re-indexes.
pub async fn index_status(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    ollama_base_url: &str,
    project_id: &str,
) -> AppResult<IndexStatus> {
    let active = resolve_config(pool, crypto, ollama_base_url).await?;
    let active_signature = EmbeddingSignatureView {
        provider: chunk_provider_string(&active),
        model: active.model.clone(),
        dimension: u32::try_from(active.dimension).unwrap_or(u32::MAX),
    };

    let signatures = chunk_repo::embedding_signatures(pool, project_id).await?;
    let embedded_chunks: u64 = signatures.iter().map(|s| s.chunk_count).sum();
    let is_stale = signatures.iter().any(|s| {
        s.provider != active_signature.provider
            || s.model != active_signature.model
            || s.dimension != active_signature.dimension
    });
    // Report the dominant signature so a mixed index (which only a bug
    // could produce — analyze rewrites the whole project) still renders.
    let indexed_with = signatures
        .into_iter()
        .max_by_key(|s| s.chunk_count)
        .map(|s| EmbeddingSignatureView {
            provider: s.provider,
            model: s.model,
            dimension: s.dimension,
        });

    Ok(IndexStatus {
        project_id: project_id.to_string(),
        embedded_chunks,
        indexed_with,
        active_config: active_signature,
        is_stale,
    })
}

/// Decrypt-and-assemble the active embedding config, falling back to
/// the implicit local-Ollama default when no row is stored.
async fn resolve_config(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    ollama_base_url: &str,
) -> AppResult<EmbeddingConfig> {
    let Some(row) = embedding_config_repo::fetch_active(pool, DEFAULT_USER_ID).await? else {
        return Ok(EmbeddingConfig {
            kind: EmbeddingProviderKind::Ollama,
            model: OLLAMA_DEFAULT_MODEL.to_string(),
            dimension: OLLAMA_DEFAULT_DIMENSION,
            base_url: Some(ollama_base_url.to_string()),
            api_key: None,
        });
    };

    let kind = EmbeddingProviderKind::from_str_value(&row.provider)?;
    let api_key = match (&row.api_key_encrypted, &row.api_key_nonce) {
        (Some(ct), Some(nonce)) => Some(crypto.decrypt_string(ct, nonce)?),
        (None, None) => resolve_llm_key_fallback(pool, crypto, kind).await?,
        _ => {
            return Err(AppError::Internal(anyhow::anyhow!(
                "embedding config key material is incomplete"
            )))
        }
    };

    Ok(EmbeddingConfig {
        kind,
        model: row.model,
        dimension: row.dimension as usize,
        base_url: row
            .base_url
            .or_else(|| ollama_fallback_base(kind, ollama_base_url)),
        api_key,
    })
}

/// Key resolution order (plan §5.1): embedding-config key first, then
/// the `user_provider_configs` row for the same provider string — so an
/// OpenAI/Gemini/Ollama-Cloud key entered for LLM use is reused without
/// re-entry. `huggingface` never matches an LLM row, so its key always
/// lives on the embedding config.
async fn resolve_api_key(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    kind: EmbeddingProviderKind,
) -> AppResult<Option<String>> {
    if let Some(row) =
        embedding_config_repo::fetch_for_user_provider(pool, DEFAULT_USER_ID, kind.as_str())
            .await?
    {
        if let (Some(ct), Some(nonce)) = (&row.api_key_encrypted, &row.api_key_nonce) {
            return Ok(Some(crypto.decrypt_string(ct, nonce)?));
        }
    }
    resolve_llm_key_fallback(pool, crypto, kind).await
}

async fn resolve_llm_key_fallback(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    kind: EmbeddingProviderKind,
) -> AppResult<Option<String>> {
    let Some(row) =
        provider_config_repo::fetch_for_user_provider(pool, DEFAULT_USER_ID, kind.as_str())
            .await?
    else {
        return Ok(None);
    };
    match (&row.api_key_encrypted, &row.api_key_nonce) {
        (Some(ct), Some(nonce)) => Ok(Some(crypto.decrypt_string(ct, nonce)?)),
        _ => Ok(None),
    }
}

/// The composite `embedding_provider` string written on every chunk —
/// format defined by [`chunk_scope_string`], kind → runtime-name
/// mapping defined by `EmbeddingProviderKind::runtime_provider_name`
/// (drift-guarded by a factory test against the concrete impls).
fn chunk_provider_string(config: &EmbeddingConfig) -> String {
    chunk_scope_string(config.kind.runtime_provider_name(), &config.model)
}

/// Local-Ollama rows (and the implicit default) fall back to the
/// app-config daemon URL; every other provider has its default base
/// baked into the factory.
fn ollama_fallback_base(kind: EmbeddingProviderKind, ollama_base_url: &str) -> Option<String> {
    matches!(kind, EmbeddingProviderKind::Ollama).then(|| ollama_base_url.to_string())
}

fn validate_model(kind: EmbeddingProviderKind, model: &str) -> AppResult<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("embedding model is empty".into()));
    }
    // HF model ids are interpolated into the endpoint URL path —
    // reject URL-breaking input here with a proper input-validation
    // error instead of letting the provider's defense-in-depth guard
    // surface it as an `LLM_*` code.
    if kind == EmbeddingProviderKind::HuggingFace
        && !crate::providers::embeddings::huggingface::is_valid_model_id(trimmed)
    {
        return Err(AppError::InvalidInput(format!(
            "invalid Hugging Face model id `{trimmed}` — model ids may only \
             contain letters, numbers, '.', '_', '-' and '/'"
        )));
    }
    Ok(trimmed.to_string())
}

fn validate_dimension(dimension: u32) -> AppResult<()> {
    if dimension == 0 || dimension > MAX_DIMENSION {
        return Err(AppError::InvalidInput(format!(
            "embedding dimension must be between 1 and {MAX_DIMENSION}, got {dimension}"
        )));
    }
    Ok(())
}

fn normalize_base_url(kind: EmbeddingProviderKind, raw: &str) -> String {
    match kind {
        EmbeddingProviderKind::Ollama | EmbeddingProviderKind::OllamaCloud => {
            normalize_ollama_base_url(raw)
        }
        EmbeddingProviderKind::OpenAi => normalize_openai_compatible_base_url(raw),
        EmbeddingProviderKind::Gemini => normalize_gemini_base_url(raw),
        // HF base URLs carry a meaningful path (`/hf-inference` on the
        // router, arbitrary mounts for TEI) — only strip the trailing
        // slash.
        EmbeddingProviderKind::HuggingFace => raw.trim_end_matches('/').to_string(),
    }
}

fn default_view() -> EmbeddingConfigView {
    EmbeddingConfigView {
        id: None,
        provider: EmbeddingProviderKind::Ollama.as_str().to_string(),
        model: OLLAMA_DEFAULT_MODEL.to_string(),
        dimension: u32::try_from(OLLAMA_DEFAULT_DIMENSION).unwrap_or(768),
        base_url: None,
        has_api_key: false,
        is_active: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-embsvc-{}.db", Uuid::new_v4()))
    }

    fn test_key() -> CryptoKey {
        CryptoKey::from_bytes([7u8; 32])
    }

    fn openai_args(api_key: Option<&str>) -> SaveEmbeddingConfigArgs {
        SaveEmbeddingConfigArgs {
            provider: "openai".into(),
            model: "text-embedding-3-small".into(),
            dimension: 1536,
            base_url: None,
            api_key: api_key.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn get_active_view_defaults_to_local_ollama() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");

        let view = get_active_view(&pool).await.expect("view");
        assert_eq!(view.id, None);
        assert_eq!(view.provider, "ollama");
        assert_eq!(view.model, "nomic-embed-text");
        assert_eq!(view.dimension, 768);
        assert!(!view.has_api_key);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn save_then_get_round_trips_and_masks_key() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let saved = save_config(&pool, &crypto, openai_args(Some("sk-embed-secret")))
            .await
            .expect("save");
        assert!(saved.has_api_key);
        assert!(saved.id.is_some());

        let view = get_active_view(&pool).await.expect("view");
        assert_eq!(view.provider, "openai");
        assert_eq!(view.model, "text-embedding-3-small");
        assert_eq!(view.dimension, 1536);
        assert!(view.has_api_key);
        let json = serde_json::to_string(&view).expect("serialize");
        assert!(!json.contains("sk-embed-secret"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn save_rejects_invalid_inputs() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let mut bad = openai_args(Some("sk-x"));
        bad.provider = "voyage".into();
        assert_eq!(
            save_config(&pool, &crypto, bad).await.expect_err("provider").code(),
            "INVALID_INPUT"
        );

        let mut bad = openai_args(Some("sk-x"));
        bad.model = "  ".into();
        assert_eq!(
            save_config(&pool, &crypto, bad).await.expect_err("model").code(),
            "INVALID_INPUT"
        );

        let mut bad = openai_args(Some("sk-x"));
        bad.dimension = 0;
        assert_eq!(
            save_config(&pool, &crypto, bad).await.expect_err("dim 0").code(),
            "INVALID_INPUT"
        );

        let mut bad = openai_args(Some("sk-x"));
        bad.dimension = MAX_DIMENSION + 1;
        assert_eq!(
            save_config(&pool, &crypto, bad).await.expect_err("dim cap").code(),
            "INVALID_INPUT"
        );

        // HF model ids are URL-path-interpolated — URL-breaking input
        // must fail as INVALID_INPUT at the service boundary, not as an
        // LLM_* code from the provider's defense-in-depth guard.
        let bad = SaveEmbeddingConfigArgs {
            provider: "huggingface".into(),
            model: "org/model?x=1".into(),
            dimension: 1024,
            base_url: None,
            api_key: Some("hf_x".into()),
        };
        assert_eq!(
            save_config(&pool, &crypto, bad).await.expect_err("hf model").code(),
            "INVALID_INPUT"
        );

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn save_preserves_existing_key_when_omitted() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        save_config(&pool, &crypto, openai_args(Some("sk-original")))
            .await
            .expect("save with key");
        let updated = save_config(&pool, &crypto, openai_args(None))
            .await
            .expect("save without key");
        assert!(updated.has_api_key, "omitted key must preserve stored key");

        let config = resolve_config(&pool, &crypto, "http://localhost:11434")
            .await
            .expect("resolve");
        assert_eq!(config.api_key.as_deref(), Some("sk-original"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn resolve_config_defaults_to_ollama_with_app_base_url() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let config = resolve_config(&pool, &crypto, "http://daemon:11434")
            .await
            .expect("resolve");
        assert_eq!(config.kind, EmbeddingProviderKind::Ollama);
        assert_eq!(config.model, OLLAMA_DEFAULT_MODEL);
        assert_eq!(config.dimension, OLLAMA_DEFAULT_DIMENSION);
        assert_eq!(config.base_url.as_deref(), Some("http://daemon:11434"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn resolve_config_falls_back_to_llm_provider_key() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        // LLM-side OpenAI key exists…
        crate::services::provider_config_service::save_config(
            &pool,
            &crypto,
            "openai".into(),
            Some("sk-llm-key".into()),
            None,
            None,
            true,
        )
        .await
        .expect("llm save");
        // …embedding config saved WITHOUT its own key.
        save_config(&pool, &crypto, openai_args(None))
            .await
            .expect("embed save");

        let config = resolve_config(&pool, &crypto, "http://localhost:11434")
            .await
            .expect("resolve");
        assert_eq!(config.api_key.as_deref(), Some("sk-llm-key"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn resolve_provider_builds_active_selection() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        save_config(&pool, &crypto, openai_args(Some("sk-x")))
            .await
            .expect("save");
        let provider = resolve_provider(&pool, &crypto, "http://localhost:11434")
            .await
            .expect("resolve");
        assert_eq!(provider.name(), "openai");
        assert_eq!(provider.model_id(), "text-embedding-3-small");
        assert_eq!(provider.dimension(), 1536);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn resolve_provider_default_is_local_ollama() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        let provider = resolve_provider(&pool, &crypto, "http://localhost:11434")
            .await
            .expect("resolve");
        assert_eq!(provider.name(), "ollama");
        assert_eq!(provider.model_id(), "nomic-embed-text");
        assert_eq!(provider.dimension(), 768);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn index_status_clean_stale_and_never_indexed() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();

        // Seed project + file for chunk FKs.
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', ?, 'p', '/tmp/p', ?, ?)",
        )
        .bind(DEFAULT_USER_ID)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO project_files (id, project_id, path, language, size_bytes, file_type, sha256, created_at, updated_at) \
             VALUES ('f1', 'p1', 'src/x.ts', 'typescript', 0, 'source', 'h', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed file");

        // Never indexed: not stale, no signature.
        let status = index_status(&pool, &crypto, "http://localhost:11434", "p1")
            .await
            .expect("status");
        assert_eq!(status.embedded_chunks, 0);
        assert!(status.indexed_with.is_none());
        assert!(!status.is_stale);
        assert_eq!(status.active_config.provider, "ollama-nomic-embed-text");

        // Index under the default signature: clean.
        chunk_repo::insert_batch(
            &pool,
            vec![chunk_repo::ChunkInsert {
                project_id: "p1".into(),
                file_id: "f1".into(),
                chunk: crate::services::chunking_service::Chunk {
                    kind: crate::services::chunking_service::ChunkKind::Function,
                    name: "fn1".into(),
                    start_line: 1,
                    end_line: 2,
                    content: "x".into(),
                    token_count: 1,
                    oversize: false,
                },
                embedding: vec![0.0; 768],
                embedding_dim: 768,
                embedding_provider: "ollama-nomic-embed-text".into(),
                embedding_model: "nomic-embed-text".into(),
            }],
        )
        .await
        .expect("insert chunk");

        let status = index_status(&pool, &crypto, "http://localhost:11434", "p1")
            .await
            .expect("status");
        assert_eq!(status.embedded_chunks, 1);
        assert!(!status.is_stale);
        let indexed = status.indexed_with.expect("signature");
        assert_eq!(indexed.model, "nomic-embed-text");
        assert_eq!(indexed.dimension, 768);

        // Switch active config to OpenAI: stale.
        save_config(&pool, &crypto, openai_args(Some("sk-x")))
            .await
            .expect("switch");
        let status = index_status(&pool, &crypto, "http://localhost:11434", "p1")
            .await
            .expect("status");
        assert!(status.is_stale);
        assert_eq!(
            status.active_config.provider,
            "openai-text-embedding-3-small"
        );

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
