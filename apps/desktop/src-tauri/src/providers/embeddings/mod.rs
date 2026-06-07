//! Embedding provider abstraction.
//!
//! Per ADR-0003, the embedding interface is parallel to `LlmProvider`
//! rather than fused: one model emits tokens, the other emits vectors.
//! Splitting keeps each trait minimal and avoids degenerate methods on
//! providers that only do one of the two jobs.
//!
//! [`OllamaEmbeddingProvider`] is the local, free default. Cloud
//! providers ship at the same shape: [`OpenAiCompatEmbeddingProvider`]
//! covers every OpenAI-wire backend (`OpenAI`, Gemini compat layer) and
//! [`HuggingFaceEmbeddingProvider`] speaks the HF feature-extraction
//! format (`plan/EMBEDDING_PROVIDER_SELECT.md`).

use async_trait::async_trait;

use super::llm::error::LlmError;

pub mod huggingface;
pub mod ollama;
pub mod openai_compat;
pub mod presets;

pub use huggingface::HuggingFaceEmbeddingProvider;
pub use ollama::OllamaEmbeddingProvider;
pub use openai_compat::OpenAiCompatEmbeddingProvider;

/// Parse a `Retry-After` header carrying whole seconds. Date-formatted
/// values (the other RFC 9110 form) are rare on embedding endpoints
/// and not worth a date dependency — they fall through to `None`.
/// Shared by the HTTP-backed providers; must be called **before** the
/// response body is consumed.
pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Compose the `embedding_provider` string written on every chunk and
/// used to scope vector searches: `{provider name}-{model}`. Single
/// definition — `analysis_service` (chunk writes), `generation_service`
/// (RAG reads), and `embedding_config_service` (stale-index detection)
/// must always agree on this format or retrieval silently breaks.
#[must_use]
pub fn chunk_scope_string(provider_name: &str, model_id: &str) -> String {
    format!("{provider_name}-{model_id}")
}

/// Provider-agnostic embedding interface. Implementations expose the
/// dimension and model identifier so downstream consumers (chunk
/// repository per ADR-0001) can persist the metadata alongside each
/// vector and refuse cross-provider comparisons.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Stable provider identifier (lowercase snake-case).
    fn name(&self) -> &'static str;

    /// Vector dimension produced by this provider/model combination.
    /// Used by `chunk_repo` to scope vector searches per-dimension
    /// (ADR-0001 "search WHERE clause must filter by ... `embedding_dim`").
    ///
    /// A dimension of `0` puts the provider in **probe mode**: per-item
    /// dimension validation is skipped so the Settings connection test
    /// can discover a model's native dimension from the first response.
    /// Probe-mode providers never reach the analysis or generation
    /// pipelines — `embedding_config_service` validates `dimension >= 1`
    /// on save and resolve.
    fn dimension(&self) -> usize;

    /// Concrete model identifier (e.g. `nomic-embed-text`,
    /// `text-embedding-3-small`). Stored on every chunk so a future
    /// model upgrade can be detected and trigger re-embedding.
    fn model_id(&self) -> &str;

    /// The chunk-scope identifier for this provider/model pair — see
    /// [`chunk_scope_string`].
    fn chunk_scope(&self) -> String {
        chunk_scope_string(self.name(), self.model_id())
    }

    /// Embed a batch of input strings. Output ordering matches input.
    ///
    /// # Errors
    ///
    /// Returns [`LlmError`] for transport, auth, rate-limit, or
    /// schema failures. Reuses the `LlmError` type since embedding
    /// providers are typically the same vendors as chat providers
    /// and surface the same failure modes.
    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, LlmError>;
}
