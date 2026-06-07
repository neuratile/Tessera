//! Ollama embedding provider — `${OLLAMA_BASE_URL}/v1/embeddings`.
//!
//! Default embedding model: `nomic-embed-text` (768 dim, Apache-2.0,
//! ships with Ollama out of the box). Wire format follows `OpenAI`'s
//! `/v1/embeddings` so the request / response shape is identical.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::EmbeddingProvider;
use crate::providers::llm::error::LlmError;
use crate::utils::provider_base_url::normalize_ollama_base_url;

/// Provider name used in `LlmError::provider` and logs.
pub const PROVIDER_NAME: &str = "ollama";

/// Default embedding model — small, fast, runs anywhere.
pub const DEFAULT_MODEL: &str = "nomic-embed-text";

/// Native dimension of `nomic-embed-text`. Other models are
/// supported via [`OllamaEmbeddingProvider::with_model`] but the
/// dimension must be supplied alongside.
pub const DEFAULT_DIMENSION: usize = 768;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

/// Ollama-backed embedding provider.
#[derive(Debug, Clone)]
pub struct OllamaEmbeddingProvider {
    base_url: String,
    model: String,
    dimension: usize,
    api_key: Option<String>,
    client: Client,
}

impl OllamaEmbeddingProvider {
    /// Construct a provider using the default `nomic-embed-text` model
    /// at 768 dimensions.
    ///
    /// # Errors
    ///
    /// Returns `LlmError::ProviderUnavailable` if the underlying HTTP
    /// client cannot be built.
    pub fn new(base_url: impl Into<String>) -> Result<Self, LlmError> {
        Self::with_model(base_url, DEFAULT_MODEL, DEFAULT_DIMENSION)
    }

    /// Construct a provider with a specific model and dimension.
    /// Caller is responsible for matching the dimension to the model
    /// (e.g. `mxbai-embed-large` is 1024). Mismatches surface as
    /// `LlmError::InvalidResponse` on the first embed call when the
    /// returned vector is not the expected length.
    ///
    /// # Errors
    ///
    /// Returns `LlmError::ProviderUnavailable` if the HTTP client
    /// cannot be built.
    pub fn with_model(
        base_url: impl Into<String>,
        model: impl Into<String>,
        dimension: usize,
    ) -> Result<Self, LlmError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| LlmError::ProviderUnavailable {
                provider: PROVIDER_NAME,
                message: format!("failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            base_url: normalize_ollama_base_url(&base_url.into()),
            model: model.into(),
            dimension,
            api_key: None,
            client,
        })
    }

    /// Attach a Bearer API key — required when the base URL points at
    /// Ollama Cloud (`https://ollama.com`) rather than a local host.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/embeddings", self.base_url)
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, LlmError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let body = OllamaEmbedRequest {
            model: &self.model,
            input: &inputs,
        };

        let mut request = self.client.post(self.endpoint()).json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| LlmError::from_reqwest(PROVIDER_NAME, &e))?;

        let status = response.status();
        if !status.is_success() {
            let preview: String = response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(256)
                .collect();
            return Err(match status.as_u16() {
                400 if preview.contains("input length")
                    || preview.contains("context length")
                    || preview.contains("exceed") =>
                {
                    // Ollama surfaces oversize-input errors as HTTP 400
                    // with `the input length exceeds the context length`.
                    // The pipeline should already truncate per-chunk
                    // input (see `analysis_service::EMBEDDING_INPUT_CHAR_CAP`);
                    // catching it here makes regressions obvious instead
                    // of opaque.
                    LlmError::InvalidResponse {
                        provider: PROVIDER_NAME,
                        message: format!(
                            "embedding input exceeds the `{}` context window. \
                             Truncate inputs upstream or switch to a model \
                             with a larger context. Raw: {preview}",
                            self.model,
                        ),
                    }
                }
                401 | 403 => LlmError::AuthFailed {
                    provider: PROVIDER_NAME,
                    message: preview,
                },
                404 => {
                    // Ollama returns 404 with `"model … not found, try
                    // pulling it first"` when the requested model isn't
                    // local. Surface a concrete `ollama pull` hint so
                    // users don't have to grep the raw HTTP body.
                    LlmError::InvalidResponse {
                        provider: PROVIDER_NAME,
                        message: format!(
                            "embedding model `{}` is not pulled locally. \
                             Run: `ollama pull {}` and retry.",
                            self.model, self.model,
                        ),
                    }
                }
                429 => LlmError::RateLimited {
                    provider: PROVIDER_NAME,
                    retry_after_seconds: None,
                },
                500..=599 => LlmError::ProviderUnavailable {
                    provider: PROVIDER_NAME,
                    message: format!("HTTP {status}: {preview}"),
                },
                _ => LlmError::InvalidResponse {
                    provider: PROVIDER_NAME,
                    message: format!("HTTP {status}: {preview}"),
                },
            });
        }

        let parsed: OllamaEmbedResponse =
            response
                .json()
                .await
                .map_err(|e| LlmError::InvalidResponse {
                    provider: PROVIDER_NAME,
                    message: format!("invalid embedding response: {e}"),
                })?;

        let mut out = Vec::with_capacity(parsed.data.len());
        for entry in parsed.data {
            // dimension 0 = probe mode (see `EmbeddingProvider::dimension`).
            if self.dimension != 0 && entry.embedding.len() != self.dimension {
                return Err(LlmError::InvalidResponse {
                    provider: PROVIDER_NAME,
                    message: format!(
                        "expected {} dimensions, got {} from model {}",
                        self.dimension,
                        entry.embedding.len(),
                        self.model
                    ),
                });
            }
            out.push(entry.embedding);
        }
        Ok(out)
    }
}

#[derive(Debug, Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    #[serde(default)]
    data: Vec<OllamaEmbedItem>,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedItem {
    #[serde(default)]
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn defaults_match_nomic_embed_text() {
        let p = OllamaEmbeddingProvider::new("http://localhost:11434").expect("provider");
        assert_eq!(p.name(), "ollama");
        assert_eq!(p.model_id(), DEFAULT_MODEL);
        assert_eq!(p.dimension(), DEFAULT_DIMENSION);
    }

    #[test]
    fn custom_model_carries_through() {
        let p = OllamaEmbeddingProvider::with_model(
            "http://localhost:11434",
            "mxbai-embed-large",
            1024,
        )
        .expect("provider");
        assert_eq!(p.model_id(), "mxbai-embed-large");
        assert_eq!(p.dimension(), 1024);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_input_returns_empty_output_without_http() {
        let provider = OllamaEmbeddingProvider::new("http://invalid:1").expect("provider");
        let out = provider.embed(Vec::new()).await.expect("empty ok");
        assert!(out.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embed_returns_vectors_against_mock() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({
            "data": [
                {"embedding": vec![0.0_f32; 768]},
                {"embedding": vec![1.0_f32; 768]}
            ]
        })
        .to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;

        let provider = OllamaEmbeddingProvider::new(server.url()).expect("provider");
        let vectors = provider
            .embed(vec!["hello".into(), "world".into()])
            .await
            .expect("embed");
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), 768);
        assert_eq!(vectors[1].len(), 768);
        assert!((vectors[1][0] - 1.0).abs() < f32::EPSILON);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embed_sends_bearer_auth_when_api_key_set() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({
            "data": [{"embedding": vec![0.0_f32; 768]}]
        })
        .to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .match_header("authorization", "Bearer oll-cloud-key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;

        let provider = OllamaEmbeddingProvider::new(server.url())
            .expect("provider")
            .with_api_key("oll-cloud-key");
        let vectors = provider.embed(vec!["hello".into()]).await.expect("embed");
        assert_eq!(vectors.len(), 1);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embed_omits_authorization_header_by_default() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({
            "data": [{"embedding": vec![0.0_f32; 768]}]
        })
        .to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;

        let provider = OllamaEmbeddingProvider::new(server.url()).expect("provider");
        let vectors = provider.embed(vec!["hello".into()]).await.expect("embed");
        assert_eq!(vectors.len(), 1);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dimension_mismatch_returns_invalid_response() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({
            "data": [{"embedding": vec![0.0_f32; 512]}]
        })
        .to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let provider = OllamaEmbeddingProvider::new(server.url()).expect("provider");
        let err = provider
            .embed(vec!["x".into()])
            .await
            .expect_err("must reject");
        assert_eq!(err.code(), "LLM_INVALID_RESPONSE");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_500_maps_to_provider_unavailable() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(503)
            .with_body("model loading")
            .create_async()
            .await;

        let provider = OllamaEmbeddingProvider::new(server.url()).expect("provider");
        let err = provider
            .embed(vec!["x".into()])
            .await
            .expect_err("must error");
        assert_eq!(err.code(), "LLM_PROVIDER_UNAVAILABLE");
        mock.assert_async().await;
    }
}
