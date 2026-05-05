//! Runtime provider selection.
//!
//! Per `rules.md` §5.2 (no SDK leaks in services) and ADR-0003 (factory
//! drives `Arc<dyn LlmProvider>`): services receive a trait object
//! resolved by the factory at startup or per-request based on user
//! configuration. The factory owns the if-else chain so service code
//! stays provider-agnostic.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::embeddings::{EmbeddingProvider, OllamaEmbeddingProvider};
use super::llm::anthropic::AnthropicProvider;
use super::llm::error::LlmError;
use super::llm::ollama::OllamaProvider;
use super::llm::openai::OpenAiProvider;
use super::llm::openrouter::OpenRouterProvider;
use super::llm::LlmProvider;
use crate::config::DEFAULT_OLLAMA_BASE_URL;

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
                .ok_or_else(missing_base_url("ollama-cloud"))?;
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
            Ok(Arc::new(OpenRouterProvider::new(key)?))
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
    }
}

/// Build an `EmbeddingProvider`. Phase 2 only ships Ollama; cloud
/// embedding providers (`OpenAI`, Voyage AI) follow at the same shape.
///
/// # Errors
///
/// See concrete provider constructors.
pub fn build_embedding_provider(
    config: &ProviderConfig,
) -> Result<Arc<dyn EmbeddingProvider>, LlmError> {
    match config.kind {
        ProviderKind::Ollama | ProviderKind::OllamaCloud => {
            let base = config
                .base_url
                .as_deref()
                .unwrap_or(DEFAULT_OLLAMA_BASE_URL);
            Ok(Arc::new(OllamaEmbeddingProvider::new(base.to_string())?))
        }
        kind => Err(LlmError::Unsupported {
            provider: provider_name_for(kind),
            feature: "embeddings",
        }),
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
    }
}

fn missing_api_key(kind: ProviderKind) -> impl FnOnce() -> LlmError {
    move || LlmError::AuthFailed {
        provider: provider_name_for(kind),
        message: "API key not configured for this provider".into(),
    }
}

fn missing_base_url(label: &'static str) -> impl FnOnce() -> LlmError {
    move || LlmError::Unsupported {
        provider: label,
        feature: "base_url",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_round_trips_through_serde() {
        let cases = [
            (ProviderKind::Ollama, "\"ollama\""),
            (ProviderKind::OllamaCloud, "\"ollama-cloud\""),
            (ProviderKind::OpenAi, "\"openai\""),
            (ProviderKind::OpenRouter, "\"openrouter\""),
            (ProviderKind::Anthropic, "\"anthropic\""),
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

    #[test]
    fn build_embedding_provider_ollama_succeeds() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Ollama,
            base_url: Some("http://localhost:11434".into()),
            api_key: None,
        };
        let (name, dim) = build_embedding_provider(&cfg)
            .map(|p| (p.name(), p.dimension()))
            .expect("ollama embed ok");
        assert_eq!(name, "ollama");
        assert_eq!(dim, 768);
    }

    #[test]
    fn build_embedding_provider_unsupported_for_anthropic() {
        let cfg = ProviderConfig {
            kind: ProviderKind::Anthropic,
            base_url: None,
            api_key: Some("k".into()),
        };
        let err = build_embedding_provider(&cfg).err().expect("must reject");
        assert_eq!(err.code(), "LLM_UNSUPPORTED");
    }

    #[test]
    fn provider_kind_as_str_matches_serde() {
        for kind in [
            ProviderKind::Ollama,
            ProviderKind::OllamaCloud,
            ProviderKind::OpenAi,
            ProviderKind::OpenRouter,
            ProviderKind::Anthropic,
        ] {
            // serde wraps the kebab-case value in JSON quotes
            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
    }
}
