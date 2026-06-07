//! Curated embedding model presets per provider.
//!
//! Single source of truth for the Settings UI: the frontend renders
//! whatever `list_embedding_presets` returns instead of hardcoding
//! model names. A "Custom…" escape hatch in the UI covers anything
//! not listed (e.g. self-hosted TEI models), with the dimension
//! discovered via the connection-test probe.

use serde::Serialize;

use crate::providers::factory::EmbeddingProviderKind;

/// One curated provider/model pair with its native dimension.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingPreset {
    pub provider: EmbeddingProviderKind,
    pub model: &'static str,
    pub dimension: u32,
    /// Default pick when the user switches to this provider.
    pub is_default: bool,
}

const fn preset(
    provider: EmbeddingProviderKind,
    model: &'static str,
    dimension: u32,
    is_default: bool,
) -> EmbeddingPreset {
    EmbeddingPreset {
        provider,
        model,
        dimension,
        is_default,
    }
}

/// Curated presets, grouped by provider. Ollama Cloud serves the same
/// model catalog as local Ollama, so it shares the list at lookup time
/// rather than duplicating rows here.
pub const PRESETS: &[EmbeddingPreset] = &[
    preset(EmbeddingProviderKind::Ollama, "nomic-embed-text", 768, true),
    preset(EmbeddingProviderKind::Ollama, "mxbai-embed-large", 1024, false),
    preset(
        EmbeddingProviderKind::Ollama,
        "snowflake-arctic-embed",
        1024,
        false,
    ),
    preset(EmbeddingProviderKind::Ollama, "all-minilm", 384, false),
    preset(EmbeddingProviderKind::Ollama, "bge-m3", 1024, false),
    preset(
        EmbeddingProviderKind::OpenAi,
        "text-embedding-3-small",
        1536,
        true,
    ),
    preset(
        EmbeddingProviderKind::OpenAi,
        "text-embedding-3-large",
        3072,
        false,
    ),
    preset(
        EmbeddingProviderKind::Gemini,
        "gemini-embedding-001",
        3072,
        true,
    ),
    preset(
        EmbeddingProviderKind::Gemini,
        "text-embedding-004",
        768,
        false,
    ),
    preset(EmbeddingProviderKind::HuggingFace, "BAAI/bge-m3", 1024, true),
    preset(
        EmbeddingProviderKind::HuggingFace,
        "sentence-transformers/all-MiniLM-L6-v2",
        384,
        false,
    ),
    preset(
        EmbeddingProviderKind::HuggingFace,
        "intfloat/multilingual-e5-large",
        1024,
        false,
    ),
    preset(
        EmbeddingProviderKind::HuggingFace,
        "BAAI/bge-large-en-v1.5",
        1024,
        false,
    ),
];

/// All presets for one provider kind. Ollama Cloud aliases the local
/// Ollama catalog.
#[must_use]
pub fn for_provider(kind: EmbeddingProviderKind) -> Vec<EmbeddingPreset> {
    let lookup = match kind {
        EmbeddingProviderKind::OllamaCloud => EmbeddingProviderKind::Ollama,
        other => other,
    };
    PRESETS
        .iter()
        .filter(|p| p.provider == lookup)
        .copied()
        .collect()
}

/// The default preset for one provider kind, used when the user
/// switches provider in the Settings UI.
#[must_use]
pub fn default_for_provider(kind: EmbeddingProviderKind) -> Option<EmbeddingPreset> {
    for_provider(kind).into_iter().find(|p| p.is_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_kind_has_exactly_one_default() {
        for kind in [
            EmbeddingProviderKind::Ollama,
            EmbeddingProviderKind::OllamaCloud,
            EmbeddingProviderKind::OpenAi,
            EmbeddingProviderKind::Gemini,
            EmbeddingProviderKind::HuggingFace,
        ] {
            let defaults = for_provider(kind)
                .into_iter()
                .filter(|p| p.is_default)
                .count();
            assert_eq!(defaults, 1, "provider {kind:?} must have one default");
        }
    }

    #[test]
    fn ollama_cloud_shares_local_catalog() {
        assert_eq!(
            for_provider(EmbeddingProviderKind::OllamaCloud).len(),
            for_provider(EmbeddingProviderKind::Ollama).len()
        );
    }

    #[test]
    fn dimensions_are_positive() {
        for p in PRESETS {
            assert!(p.dimension > 0, "preset {} has zero dimension", p.model);
        }
    }

    #[test]
    fn preset_serializes_to_camel_case() {
        let json = serde_json::to_value(
            default_for_provider(EmbeddingProviderKind::OpenAi).expect("default"),
        )
        .expect("serialize");
        assert_eq!(json["model"], "text-embedding-3-small");
        assert_eq!(json["isDefault"], true);
        assert_eq!(json["provider"], "openai");
        assert_eq!(json["dimension"], 1536);
    }
}
