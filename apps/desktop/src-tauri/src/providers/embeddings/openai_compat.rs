//! Generic OpenAI-wire embedding provider.
//!
//! `OpenAI`, Gemini (via its `OpenAI`-compatibility layer), and Ollama all
//! speak the same `/embeddings` request/response shape, so one impl
//! covers every OpenAI-compatible cloud backend. The factory supplies
//! the fully-built endpoint URL and a static provider name; this module
//! only owns the wire format and error mapping.
//!
//! Ollama keeps its dedicated [`super::OllamaEmbeddingProvider`] because
//! it layers daemon-specific hints (`ollama pull`, context-window 400s)
//! on top of the same wire format.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{parse_retry_after, EmbeddingProvider};
use crate::providers::llm::error::LlmError;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

/// OpenAI-wire embedding provider for cloud backends.
#[derive(Debug, Clone)]
pub struct OpenAiCompatEmbeddingProvider {
    provider_name: &'static str,
    endpoint: String,
    model: String,
    dimension: usize,
    /// Serialized as the `OpenAI` `dimensions` request field. Only
    /// `text-embedding-3-*` accepts it; the factory decides when to
    /// set it (other models reject unknown fields or ignore them).
    request_dimensions: Option<u32>,
    api_key: String,
    client: Client,
}

impl OpenAiCompatEmbeddingProvider {
    /// Construct a provider posting to `endpoint` (a fully-built
    /// `…/embeddings` URL — the factory owns base-URL + path joining).
    ///
    /// `dimension == 0` enables probe mode: per-item dimension
    /// validation is skipped so the connection-test flow can discover
    /// the model's native dimension (see `EmbeddingProvider` docs).
    ///
    /// # Errors
    ///
    /// Returns `LlmError::ProviderUnavailable` if the HTTP client
    /// cannot be built, or `LlmError::AuthFailed` when `api_key` is
    /// empty.
    pub fn new(
        provider_name: &'static str,
        endpoint: impl Into<String>,
        model: impl Into<String>,
        dimension: usize,
        request_dimensions: Option<u32>,
        api_key: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(LlmError::AuthFailed {
                provider: provider_name,
                message: "API key not configured for this provider".into(),
            });
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| LlmError::ProviderUnavailable {
                provider: provider_name,
                message: format!("failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            provider_name,
            endpoint: endpoint.into(),
            model: model.into(),
            dimension,
            request_dimensions,
            api_key,
            client,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiCompatEmbeddingProvider {
    fn name(&self) -> &'static str {
        self.provider_name
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
        let input_count = inputs.len();

        let body = EmbedRequest {
            model: &self.model,
            input: &inputs,
            dimensions: self.request_dimensions,
        };

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::from_reqwest(self.provider_name, &e))?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let preview: String = response
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(256)
                .collect();
            return Err(match status.as_u16() {
                401 | 403 => LlmError::AuthFailed {
                    provider: self.provider_name,
                    message: preview,
                },
                404 => LlmError::InvalidResponse {
                    provider: self.provider_name,
                    message: format!(
                        "embedding model `{}` was not found at this endpoint. \
                         Check the model id and base URL. Raw: {preview}",
                        self.model,
                    ),
                },
                429 => LlmError::RateLimited {
                    provider: self.provider_name,
                    retry_after_seconds: retry_after,
                },
                500..=599 => LlmError::ProviderUnavailable {
                    provider: self.provider_name,
                    message: format!("HTTP {status}: {preview}"),
                },
                _ => LlmError::InvalidResponse {
                    provider: self.provider_name,
                    message: format!("HTTP {status}: {preview}"),
                },
            });
        }

        let parsed: EmbedResponse = response
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse {
                provider: self.provider_name,
                message: format!("invalid embedding response: {e}"),
            })?;

        if parsed.data.len() != input_count {
            return Err(LlmError::InvalidResponse {
                provider: self.provider_name,
                message: format!(
                    "expected {input_count} embeddings, got {} from model {}",
                    parsed.data.len(),
                    self.model
                ),
            });
        }

        let mut out = Vec::with_capacity(parsed.data.len());
        for entry in parsed.data {
            if self.dimension != 0 && entry.embedding.len() != self.dimension {
                return Err(LlmError::InvalidResponse {
                    provider: self.provider_name,
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
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    #[serde(default)]
    data: Vec<EmbedItem>,
}

#[derive(Debug, Deserialize)]
struct EmbedItem {
    #[serde(default)]
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn provider_at(endpoint: String, dimension: usize) -> OpenAiCompatEmbeddingProvider {
        OpenAiCompatEmbeddingProvider::new(
            "openai",
            endpoint,
            "text-embedding-3-small",
            dimension,
            None,
            "sk-test",
        )
        .expect("provider")
    }

    #[test]
    fn rejects_empty_api_key() {
        let err = OpenAiCompatEmbeddingProvider::new(
            "openai",
            "https://api.openai.com/v1/embeddings",
            "text-embedding-3-small",
            1536,
            None,
            "  ",
        )
        .expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn accessors_report_configured_values() {
        let p = provider_at("https://api.openai.com/v1/embeddings".into(), 1536);
        assert_eq!(p.name(), "openai");
        assert_eq!(p.model_id(), "text-embedding-3-small");
        assert_eq!(p.dimension(), 1536);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_input_returns_empty_output_without_http() {
        let p = provider_at("http://invalid:1/v1/embeddings".into(), 4);
        let out = p.embed(Vec::new()).await.expect("empty ok");
        assert!(out.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embed_returns_vectors_and_sends_bearer() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({
            "data": [
                {"embedding": vec![0.0_f32; 4]},
                {"embedding": vec![1.0_f32; 4]}
            ]
        })
        .to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .match_header("authorization", "Bearer sk-test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        let vectors = p
            .embed(vec!["hello".into(), "world".into()])
            .await
            .expect("embed");
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), 4);
        assert!((vectors[1][0] - 1.0).abs() < f32::EPSILON);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dimensions_param_serialized_only_when_set() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({"data": [{"embedding": vec![0.0_f32; 8]}]}).to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::PartialJson(serde_json::json!({"dimensions": 8})),
            ]))
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let p = OpenAiCompatEmbeddingProvider::new(
            "openai",
            format!("{}/v1/embeddings", server.url()),
            "text-embedding-3-small",
            8,
            Some(8),
            "sk-test",
        )
        .expect("provider");
        p.embed(vec!["x".into()]).await.expect("embed");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dimensions_param_omitted_when_none() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({"data": [{"embedding": vec![0.0_f32; 4]}]}).to_string();
        // Exact-body matcher: a `dimensions` key in the payload would
        // fail the match, proving `skip_serializing_if` drops it.
        let mock = server
            .mock("POST", "/v1/embeddings")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "model": "text-embedding-3-small",
                "input": ["x"]
            })))
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        p.embed(vec!["x".into()]).await.expect("embed");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn probe_mode_dimension_zero_skips_validation() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({"data": [{"embedding": vec![0.0_f32; 1536]}]}).to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 0);
        let vectors = p.embed(vec!["probe".into()]).await.expect("probe ok");
        assert_eq!(vectors[0].len(), 1536);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dimension_mismatch_returns_invalid_response() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({"data": [{"embedding": vec![0.0_f32; 512]}]}).to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must reject");
        assert_eq!(err.code(), "LLM_INVALID_RESPONSE");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn count_mismatch_returns_invalid_response() {
        let mut server = Server::new_async().await;
        let body = serde_json::json!({"data": [{"embedding": vec![0.0_f32; 4]}]}).to_string();
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        let err = p
            .embed(vec!["a".into(), "b".into()])
            .await
            .expect_err("must reject");
        assert_eq!(err.code(), "LLM_INVALID_RESPONSE");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_401_maps_to_auth_failed() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(401)
            .with_body("invalid api key")
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must error");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_429_carries_retry_after_seconds() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(429)
            .with_header("retry-after", "30")
            .with_body("rate limited")
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must error");
        match err {
            LlmError::RateLimited {
                retry_after_seconds,
                ..
            } => assert_eq!(retry_after_seconds, Some(30)),
            other => panic!("expected RateLimited, got {other:?}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_500_maps_to_provider_unavailable() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;

        let p = provider_at(format!("{}/v1/embeddings", server.url()), 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must error");
        assert_eq!(err.code(), "LLM_PROVIDER_UNAVAILABLE");
        mock.assert_async().await;
    }
}
