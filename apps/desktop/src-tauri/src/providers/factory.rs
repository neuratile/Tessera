//! Runtime provider selection.
//!
//! Per `rules.md` §5.2 (no SDK leaks in services) and ADR-0003 (factory
//! drives `Arc<dyn LlmProvider>`): services receive a trait object
//! resolved by the factory at startup or per-request based on user
//! configuration. The factory owns the if-else chain so service code
//! stays provider-agnostic.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::embeddings::{
    EmbeddingProvider, HuggingFaceEmbeddingProvider, OllamaEmbeddingProvider,
    OpenAiCompatEmbeddingProvider,
};
use super::llm::anthropic::AnthropicProvider;
use super::llm::error::LlmError;
use super::llm::gemini::GeminiProvider;
use super::llm::ollama::OllamaProvider;
use super::llm::openai::OpenAiProvider;
use super::llm::openrouter::OpenRouterProvider;
use super::llm::LlmProvider;
use crate::config::{DEFAULT_OLLAMA_BASE_URL, DEFAULT_OLLAMA_CLOUD_BASE_URL};
use crate::error::{AppError, AppResult};

/// Discriminator used to match the provider kind selected by the user
/// in the Settings UI. Stored on `user_provider_configs.provider`.
///
/// Each variant has an explicit `serde(rename = ...)` instead of a
/// blanket `kebab-case` so multi-word labels like `openai` and
/// `openrouter` stay as one token rather than `open-ai` /
/// `open-router`. The string identifier returned by [`Self::as_str`]
/// is the same one written to disk and over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "ollama-cloud")]
    OllamaCloud,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
}

impl ProviderKind {
    /// Stable string used in DB rows and IPC payloads. Mirrors the
    /// kebab-case serde representation so the round-trip is lossless.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OllamaCloud => "ollama-cloud",
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    /// Parse the stable wire/database string used across IPC and `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns `AppError::InvalidInput` when `value` does not match one of the
    /// supported provider identifiers.
    pub fn from_str_value(value: &str) -> AppResult<Self> {
        match value.trim() {
            "ollama" => Ok(Self::Ollama),
            "ollama-cloud" => Ok(Self::OllamaCloud),
            "openai" => Ok(Self::OpenAi),
            "openrouter" => Ok(Self::OpenRouter),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            _ => Err(AppError::InvalidInput(format!(
                "unknown provider kind `{value}`"
            ))),
        }
    }

    /// Whether this provider needs an API key. Ollama Local is the
    /// only `false` — every cloud provider requires a key.
    #[must_use]
    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }
}

/// User-supplied configuration assembled by the Settings command
/// before being handed to the factory. `api_key` is decrypted just
/// before construction; it is never stored on disk in plaintext per
/// `rules.md` §9.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

/// Build an `LlmProvider` matching `config.kind`. Cheap to call: each
/// provider holds an HTTP client behind an `Arc` so `Clone` is shallow.
///
/// # Errors
///
/// Returns `LlmError::Unsupported` if the kind requires an API key
/// and none was supplied. Otherwise returns whatever the concrete
/// constructor returns (typically `LlmError::AuthFailed` for empty
/// keys or `LlmError::ProviderUnavailable` for HTTP-client setup
/// failures).
pub fn build_llm_provider(config: &ProviderConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    match config.kind {
        ProviderKind::Ollama => {
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(DEFAULT_OLLAMA_BASE_URL);
            Ok(Arc::new(OllamaProvider::new(base.to_string())?))
        }
        ProviderKind::OllamaCloud => {
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(DEFAULT_OLLAMA_CLOUD_BASE_URL);
            // Ollama Cloud is OpenAI-compatible; reuse OpenAiProvider
            // so the auth path is the same as cloud OpenAI.
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_api_key(ProviderKind::OllamaCloud))?;
            Ok(Arc::new(OpenAiProvider::with_base_url(key, base)?))
        }
        ProviderKind::OpenAi => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_api_key(ProviderKind::OpenAi))?;
            let provider = if let Some(base) = config.base_url.as_deref() {
                OpenAiProvider::with_base_url(key, base)?
            } else {
                OpenAiProvider::new(key)?
            };
            Ok(Arc::new(provider))
        }
        ProviderKind::OpenRouter => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_api_key(ProviderKind::OpenRouter))?;
            let provider = if let Some(base) = config.base_url.as_deref() {
                OpenRouterProvider::with_base_url(key, base)?
            } else {
                OpenRouterProvider::new(key)?
            };
            Ok(Arc::new(provider))
        }
        ProviderKind::Anthropic => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_api_key(ProviderKind::Anthropic))?;
            let provider = if let Some(base) = config.base_url.as_deref() {
                AnthropicProvider::with_base_url(key, base)?
            } else {
                AnthropicProvider::new(key)?
            };
            Ok(Arc::new(provider))
        }
        ProviderKind::Gemini => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_api_key(ProviderKind::Gemini))?;
            let provider = if let Some(base) = config.base_url.as_deref() {
                GeminiProvider::with_base_url(key, base)?
            } else {
                GeminiProvider::new(key)?
            };
            Ok(Arc::new(provider))
        }
    }
}

/// Discriminator for the embedding provider selected in the Settings
/// UI. Stored on `user_embedding_configs.provider`. Deliberately
/// separate from [`ProviderKind`]: the LLM catalog (Anthropic,
/// `OpenRouter`) and the embedding catalog (Hugging Face) diverge, and
/// fusing them would force permanent `Unsupported` arms in both
/// directions (`plan/EMBEDDING_PROVIDER_SELECT.md` §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingProviderKind {
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "ollama-cloud")]
    OllamaCloud,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "huggingface")]
    HuggingFace,
}

impl EmbeddingProviderKind {
    /// Stable string used in DB rows and IPC payloads. Mirrors the
    /// serde representation so the round-trip is lossless.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OllamaCloud => "ollama-cloud",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::HuggingFace => "huggingface",
        }
    }

    /// Parse the stable wire/database string used across IPC and `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns `AppError::InvalidInput` when `value` does not match one
    /// of the supported embedding provider identifiers.
    pub fn from_str_value(value: &str) -> AppResult<Self> {
        match value.trim() {
            "ollama" => Ok(Self::Ollama),
            "ollama-cloud" => Ok(Self::OllamaCloud),
            "openai" => Ok(Self::OpenAi),
            "gemini" => Ok(Self::Gemini),
            "huggingface" => Ok(Self::HuggingFace),
            _ => Err(AppError::InvalidInput(format!(
                "unknown embedding provider kind `{value}`"
            ))),
        }
    }

    /// Whether this provider needs an API key. Local Ollama is the
    /// only `false`.
    #[must_use]
    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    /// The runtime `EmbeddingProvider::name()` the built provider will
    /// report — Ollama Cloud reuses the local Ollama impl, so both map
    /// to `"ollama"`. Single definition of the kind → runtime-name
    /// mapping; `embedding_config_service` composes chunk-scope strings
    /// from it without building a provider (no key needed). Kept honest
    /// by the `runtime_name_matches_built_provider` test below.
    #[must_use]
    pub fn runtime_provider_name(self) -> &'static str {
        match self {
            Self::Ollama | Self::OllamaCloud => "ollama",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::HuggingFace => "huggingface",
        }
    }
}

/// Resolved embedding configuration handed to the factory by
/// `embedding_config_service`. `api_key` is decrypted just before
/// construction and never persisted in plaintext (`rules.md` §9).
#[derive(Clone)]
pub struct EmbeddingConfig {
    pub kind: EmbeddingProviderKind,
    pub model: String,
    /// `0` builds the provider in probe mode (dimension discovery for
    /// the Settings connection test) — see `EmbeddingProvider::dimension`.
    pub dimension: usize,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for EmbeddingConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manual impl so a decrypted API key can never leak through
        // `{:?}` logging (rules.md §5.4).
        f.debug_struct("EmbeddingConfig")
            .field("kind", &self.kind)
            .field("model", &self.model)
            .field("dimension", &self.dimension)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Build an `EmbeddingProvider` matching `config.kind`.
///
/// # Errors
///
/// Returns `LlmError::AuthFailed` when the kind requires an API key
/// and none was supplied; otherwise whatever the concrete constructor
/// returns.
pub fn build_embedding_provider(
    config: &EmbeddingConfig,
) -> Result<Arc<dyn EmbeddingProvider>, LlmError> {
    match config.kind {
        EmbeddingProviderKind::Ollama => {
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(DEFAULT_OLLAMA_BASE_URL);
            Ok(Arc::new(OllamaEmbeddingProvider::with_model(
                base,
                &config.model,
                config.dimension,
            )?))
        }
        EmbeddingProviderKind::OllamaCloud => {
            // Same fallback rule as `build_llm_provider`: no stored
            // base URL means the official cloud endpoint, never the
            // localhost default. ollama.com requires a Bearer key.
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(DEFAULT_OLLAMA_CLOUD_BASE_URL);
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_embedding_api_key(EmbeddingProviderKind::OllamaCloud))?;
            Ok(Arc::new(
                OllamaEmbeddingProvider::with_model(base, &config.model, config.dimension)?
                    .with_api_key(key),
            ))
        }
        EmbeddingProviderKind::OpenAi => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_embedding_api_key(EmbeddingProviderKind::OpenAi))?;
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(super::llm::openai::DEFAULT_BASE_URL);
            // Only `text-embedding-3-*` accepts the `dimensions`
            // request field; probe mode (dimension 0) never sends it.
            let request_dimensions = (config.model.starts_with("text-embedding-3")
                && config.dimension > 0)
                .then(|| u32::try_from(config.dimension).unwrap_or(u32::MAX));
            Ok(Arc::new(OpenAiCompatEmbeddingProvider::new(
                "openai",
                format!("{}/v1/embeddings", base.trim_end_matches('/')),
                &config.model,
                config.dimension,
                request_dimensions,
                key,
            )?))
        }
        EmbeddingProviderKind::Gemini => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_embedding_api_key(EmbeddingProviderKind::Gemini))?;
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(super::llm::gemini::DEFAULT_BASE_URL);
            Ok(Arc::new(OpenAiCompatEmbeddingProvider::new(
                "gemini",
                format!("{}/v1beta/openai/embeddings", base.trim_end_matches('/')),
                &config.model,
                config.dimension,
                None,
                key,
            )?))
        }
        EmbeddingProviderKind::HuggingFace => {
            let key = config
                .api_key
                .as_deref()
                .ok_or_else(missing_embedding_api_key(EmbeddingProviderKind::HuggingFace))?;
            let provider =
                HuggingFaceEmbeddingProvider::new(key, &config.model, config.dimension)?;
            Ok(Arc::new(match config.base_url.as_deref() {
                Some(base) => provider.with_base_url(base),
                None => provider,
            }))
        }
    }
}

fn missing_embedding_api_key(kind: EmbeddingProviderKind) -> impl FnOnce() -> LlmError {
    move || LlmError::AuthFailed {
        provider: kind.as_str(),
        message: "API key not configured for this provider".into(),
    }
}

fn provider_name_for(kind: ProviderKind) -> &'static str {
    // Static-string lookup so `LlmError::Unsupported::provider`
    // (which is `&'static str`) stays sound across all variants.
    match kind {
        ProviderKind::Ollama | ProviderKind::OllamaCloud => "ollama",
        ProviderKind::OpenAi => "openai",
        ProviderKind::OpenRouter => "openrouter",
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::Gemini => "gemini",
    }
}

fn missing_api_key(kind: ProviderKind) -> impl FnOnce() -> LlmError {
    move || LlmError::AuthFailed {
        provider: provider_name_for(kind),
        message: "API key not configured for this provider".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::llm::types::{Chunk, GenerateRequest, Message};
    use futures::StreamExt;
    use mockito::Server;

    #[test]
    fn provider_kind_round_trips_through_serde() {
        let cases = [
            (ProviderKind::Ollama, "\"ollama\""),
            (ProviderKind::OllamaCloud, "\"ollama-cloud\""),
            (ProviderKind::OpenAi, "\"openai\""),
            (ProviderKind::OpenRouter, "\"openrouter\""),
            (ProviderKind::Anthropic, "\"anthropic\""),
            (ProviderKind::Gemini, "\"gemini\""),
        ];
        for (kind, expected) in cases {
            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, expected);
            let back: ProviderKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn requires_api_key_only_false_for_local_ollama() {
        assert!(!ProviderKind::Ollama.requires_api_key());
        assert!(ProviderKind::OllamaCloud.requires_api_key());
        assert!(ProviderKind::OpenAi.requires_api_key());
        assert!(ProviderKind::OpenRouter.requires_api_key());
        assert!(ProviderKind::Anthropic.requires_api_key());
        assert!(ProviderKind::Gemini.requires_api_key());
    }

    #[test]
    fn build_llm_provider_ollama_works_without_api_key() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Ollama,
            base_url: Some("http://localhost:11434".into()),
            api_key: None,
        };
        let name = build_llm_provider(&cfg)
            .map(|p| p.name())
            .expect("ollama always builds");
        assert_eq!(name, "ollama");
    }

    #[test]
    fn build_llm_provider_ollama_falls_back_to_default_url() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Ollama,
            base_url: None,
            api_key: None,
        };
        let name = build_llm_provider(&cfg)
            .map(|p| p.name())
            .expect("default url ok");
        assert_eq!(name, "ollama");
    }

    #[test]
    fn build_llm_provider_ollama_cloud_falls_back_to_default_url() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OllamaCloud,
            base_url: None,
            api_key: Some("oll-test".into()),
        };
        let name = build_llm_provider(&cfg)
            .map(|p| p.name())
            .expect("default cloud url ok");
        assert_eq!(name, "openai");
    }

    #[test]
    fn build_llm_provider_ollama_cloud_requires_api_key() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OllamaCloud,
            base_url: None,
            api_key: None,
        };
        let err = build_llm_provider(&cfg).err().expect("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        assert_eq!(err.provider(), "ollama");
    }

    #[test]
    fn build_llm_provider_openai_requires_api_key() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OpenAi,
            base_url: None,
            api_key: None,
        };
        let err = build_llm_provider(&cfg).err().expect("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        assert_eq!(err.provider(), "openai");
    }

    #[test]
    fn build_llm_provider_anthropic_requires_api_key() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Anthropic,
            base_url: None,
            api_key: None,
        };
        let err = build_llm_provider(&cfg).err().expect("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        assert_eq!(err.provider(), "anthropic");
    }

    #[test]
    fn build_llm_provider_gemini_requires_api_key() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Gemini,
            base_url: None,
            api_key: None,
        };
        let err = build_llm_provider(&cfg).err().expect("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        assert_eq!(err.provider(), "gemini");
    }

    #[test]
    fn build_llm_provider_gemini_with_key_succeeds() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Gemini,
            base_url: None,
            api_key: Some("AIza-test".into()),
        };
        let name = build_llm_provider(&cfg)
            .map(|p| p.name())
            .expect("gemini ok");
        assert_eq!(name, "gemini");
    }

    #[test]
    fn build_llm_provider_openrouter_with_key_succeeds() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OpenRouter,
            base_url: None,
            api_key: Some("sk-or-test".into()),
        };
        let name = build_llm_provider(&cfg)
            .map(|p| p.name())
            .expect("openrouter ok");
        assert_eq!(name, "openrouter");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn build_llm_provider_openrouter_honors_custom_base_url() {
        let mut server = Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("authorization", "Bearer sk-or-test")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let cfg = ProviderConfig {
            kind: ProviderKind::OpenRouter,
            base_url: Some(server.url()),
            api_key: Some("sk-or-test".into()),
        };
        let provider = build_llm_provider(&cfg).expect("custom base ok");
        let request = GenerateRequest {
            model: "qwen/qwen2.5-coder-32b-instruct".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        };

        let mut stream = provider.stream(request);
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Chunk::TextDelta(delta) = chunk.expect("chunk") {
                text.push_str(&delta);
            }
        }

        assert_eq!(text, "ok");
        mock.assert_async().await;
    }

    #[test]
    fn build_llm_provider_openai_custom_base_url() {
        let cfg = ProviderConfig {
            kind: ProviderKind::OpenAi,
            base_url: Some("https://gateway.example.com".into()),
            api_key: Some("sk-test".into()),
        };
        let name = build_llm_provider(&cfg)
            .map(|p| p.name())
            .expect("custom base ok");
        assert_eq!(name, "openai");
    }

    fn embedding_cfg(kind: EmbeddingProviderKind, api_key: Option<&str>) -> EmbeddingConfig {
        EmbeddingConfig {
            kind,
            model: "test-model".into(),
            dimension: 768,
            base_url: None,
            api_key: api_key.map(str::to_string),
        }
    }

    #[test]
    fn embedding_provider_kind_round_trips_through_serde() {
        let cases = [
            (EmbeddingProviderKind::Ollama, "\"ollama\""),
            (EmbeddingProviderKind::OllamaCloud, "\"ollama-cloud\""),
            (EmbeddingProviderKind::OpenAi, "\"openai\""),
            (EmbeddingProviderKind::Gemini, "\"gemini\""),
            (EmbeddingProviderKind::HuggingFace, "\"huggingface\""),
        ];
        for (kind, expected) in cases {
            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, expected);
            let back: EmbeddingProviderKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, kind);
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(
                EmbeddingProviderKind::from_str_value(kind.as_str()).expect("parse"),
                kind
            );
        }
    }

    #[test]
    fn embedding_provider_kind_from_str_value_rejects_unknown() {
        let err =
            EmbeddingProviderKind::from_str_value("voyage").expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn embedding_requires_api_key_only_false_for_local_ollama() {
        assert!(!EmbeddingProviderKind::Ollama.requires_api_key());
        assert!(EmbeddingProviderKind::OllamaCloud.requires_api_key());
        assert!(EmbeddingProviderKind::OpenAi.requires_api_key());
        assert!(EmbeddingProviderKind::Gemini.requires_api_key());
        assert!(EmbeddingProviderKind::HuggingFace.requires_api_key());
    }

    #[test]
    fn build_embedding_provider_ollama_works_without_api_key() {
        let mut cfg = embedding_cfg(EmbeddingProviderKind::Ollama, None);
        cfg.model = "nomic-embed-text".into();
        let (name, dim, model) = build_embedding_provider(&cfg)
            .map(|p| (p.name(), p.dimension(), p.model_id().to_string()))
            .expect("ollama embed ok");
        assert_eq!(name, "ollama");
        assert_eq!(dim, 768);
        assert_eq!(model, "nomic-embed-text");
    }

    #[test]
    fn build_embedding_provider_requires_key_for_cloud_kinds() {
        for kind in [
            EmbeddingProviderKind::OllamaCloud,
            EmbeddingProviderKind::OpenAi,
            EmbeddingProviderKind::Gemini,
            EmbeddingProviderKind::HuggingFace,
        ] {
            let err = build_embedding_provider(&embedding_cfg(kind, None))
                .err()
                .expect("must reject");
            assert_eq!(err.code(), "LLM_AUTH_FAILED", "kind {kind:?}");
        }
    }

    #[test]
    fn build_embedding_provider_openai_with_key_succeeds() {
        let mut cfg = embedding_cfg(EmbeddingProviderKind::OpenAi, Some("sk-test"));
        cfg.model = "text-embedding-3-small".into();
        cfg.dimension = 1536;
        let (name, dim) = build_embedding_provider(&cfg)
            .map(|p| (p.name(), p.dimension()))
            .expect("openai embed ok");
        assert_eq!(name, "openai");
        assert_eq!(dim, 1536);
    }

    #[test]
    fn build_embedding_provider_gemini_with_key_succeeds() {
        let mut cfg = embedding_cfg(EmbeddingProviderKind::Gemini, Some("AIza-test"));
        cfg.model = "gemini-embedding-001".into();
        cfg.dimension = 3072;
        let name = build_embedding_provider(&cfg)
            .map(|p| p.name())
            .expect("gemini embed ok");
        assert_eq!(name, "gemini");
    }

    #[test]
    fn build_embedding_provider_huggingface_with_key_succeeds() {
        let mut cfg = embedding_cfg(EmbeddingProviderKind::HuggingFace, Some("hf_test"));
        cfg.model = "BAAI/bge-m3".into();
        cfg.dimension = 1024;
        let (name, model) = build_embedding_provider(&cfg)
            .map(|p| (p.name(), p.model_id().to_string()))
            .expect("hf embed ok");
        assert_eq!(name, "huggingface");
        assert_eq!(model, "BAAI/bge-m3");
    }

    #[test]
    fn runtime_name_matches_built_provider() {
        // Guards the kind → runtime-name mapping against drifting from
        // what the concrete impls actually report via `name()`.
        for kind in [
            EmbeddingProviderKind::Ollama,
            EmbeddingProviderKind::OllamaCloud,
            EmbeddingProviderKind::OpenAi,
            EmbeddingProviderKind::Gemini,
            EmbeddingProviderKind::HuggingFace,
        ] {
            let provider = build_embedding_provider(&embedding_cfg(kind, Some("test-key")))
                .expect("provider builds");
            assert_eq!(
                provider.name(),
                kind.runtime_provider_name(),
                "kind {kind:?}"
            );
        }
    }

    #[test]
    fn embedding_config_debug_redacts_api_key() {
        let cfg = embedding_cfg(EmbeddingProviderKind::OpenAi, Some("sk-super-secret"));
        let debug = format!("{cfg:?}");
        assert!(!debug.contains("sk-super-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn provider_kind_as_str_matches_serde() {
        for kind in [
            ProviderKind::Ollama,
            ProviderKind::OllamaCloud,
            ProviderKind::OpenAi,
            ProviderKind::OpenRouter,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
        ] {
            // serde wraps the kebab-case value in JSON quotes
            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
    }

    #[test]
    fn provider_kind_from_str_value_matches_as_str() {
        for kind in [
            ProviderKind::Ollama,
            ProviderKind::OllamaCloud,
            ProviderKind::OpenAi,
            ProviderKind::OpenRouter,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
        ] {
            let parsed = ProviderKind::from_str_value(kind.as_str()).expect("parse");
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn provider_kind_from_str_value_rejects_unknown() {
        let err = ProviderKind::from_str_value("unknown-provider").expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }
}
