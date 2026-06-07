//! Hugging Face Inference API embedding provider.
//!
//! Cloud-only (`plan/EMBEDDING_PROVIDER_SELECT.md`): posts to the
//! feature-extraction pipeline of the HF inference router. Unlike the
//! `OpenAI` wire format, the model id rides in the URL path, the body is
//! `{"inputs": [...]}`, and the response is a bare nested array of
//! vectors. Self-hosted text-embeddings-inference (TEI) deployments
//! work through [`HuggingFaceEmbeddingProvider::with_base_url`].

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;

use super::{parse_retry_after, EmbeddingProvider};
use crate::providers::llm::error::LlmError;

/// Provider name used in `LlmError::provider` and chunk metadata.
pub const PROVIDER_NAME: &str = "huggingface";

/// HF inference router root. The feature-extraction path is appended
/// per-model: `{base}/models/{model}/pipeline/feature-extraction`.
pub const DEFAULT_BASE_URL: &str = "https://router.huggingface.co/hf-inference";

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

/// Hugging Face Inference API embedding provider.
#[derive(Debug, Clone)]
pub struct HuggingFaceEmbeddingProvider {
    base_url: String,
    model: String,
    dimension: usize,
    api_key: String,
    client: Client,
}

impl HuggingFaceEmbeddingProvider {
    /// Construct a provider against the public HF inference router.
    ///
    /// `dimension == 0` enables probe mode (validation skipped — see
    /// `EmbeddingProvider` docs).
    ///
    /// # Errors
    ///
    /// - `LlmError::AuthFailed` when `api_key` is empty.
    /// - `LlmError::InvalidResponse` when the model id contains
    ///   URL-breaking characters (it is path-interpolated).
    /// - `LlmError::ProviderUnavailable` if the HTTP client cannot be
    ///   built.
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        dimension: usize,
    ) -> Result<Self, LlmError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(LlmError::AuthFailed {
                provider: PROVIDER_NAME,
                message: "API key not configured for this provider".into(),
            });
        }
        let model = model.into();
        validate_model_id(&model)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| LlmError::ProviderUnavailable {
                provider: PROVIDER_NAME,
                message: format!("failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model,
            dimension,
            api_key,
            client,
        })
    }

    /// Point at a self-hosted TEI deployment or proxy instead of the
    /// public router.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/models/{}/pipeline/feature-extraction",
            self.base_url, self.model
        )
    }
}

/// Whether `model` is a well-formed HF model id (`[A-Za-z0-9._/-]`, no
/// `..` traversal). The id is interpolated into the endpoint path, so
/// anything that could splice the URL (`?`, `#`, whitespace) is
/// rejected. Exposed so `embedding_config_service` can validate user
/// input up front with a proper `AppError::InvalidInput` — the
/// constructor guard below is defense in depth, not the primary
/// validation surface.
#[must_use]
pub fn is_valid_model_id(model: &str) -> bool {
    !model.is_empty()
        && !model.contains("..")
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

fn validate_model_id(model: &str) -> Result<(), LlmError> {
    if is_valid_model_id(model) {
        Ok(())
    } else {
        // `LlmError` has no input-validation variant; this guard is
        // unreachable through the IPC path (the service rejects bad
        // ids with `AppError::InvalidInput` first), so the message
        // states explicitly that no request was sent.
        Err(LlmError::InvalidResponse {
            provider: PROVIDER_NAME,
            message: format!(
                "invalid Hugging Face model id `{model}` — rejected before \
                 any request was sent. Model ids may only contain letters, \
                 numbers, '.', '_', '-' and '/'.",
            ),
        })
    }
}

#[async_trait]
impl EmbeddingProvider for HuggingFaceEmbeddingProvider {
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
        let input_count = inputs.len();

        // Always send `inputs` as an array: a bare-string input makes
        // some models answer with a single flat vector instead of a
        // nested array, which would break response decoding.
        let body = HfEmbedRequest {
            inputs: &inputs,
            normalize: true,
            truncate: true,
        };

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::from_reqwest(PROVIDER_NAME, &e))?;

        let status = response.status();
        if !status.is_success() {
            // Headers must be read before `.text()` consumes the response.
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
                    provider: PROVIDER_NAME,
                    message: preview,
                },
                404 => LlmError::InvalidResponse {
                    provider: PROVIDER_NAME,
                    message: format!(
                        "model `{}` was not found on the Hugging Face inference \
                         router. Check the model id (org/name) and that it \
                         supports feature-extraction. Raw: {preview}",
                        self.model,
                    ),
                },
                429 => LlmError::RateLimited {
                    provider: PROVIDER_NAME,
                    retry_after_seconds: retry_after,
                },
                503 if preview.contains("loading") || preview.contains("estimated_time") => {
                    // Cold models return 503 with an estimated warm-up
                    // time while HF spins up the inference container.
                    LlmError::ProviderUnavailable {
                        provider: PROVIDER_NAME,
                        message: format!(
                            "model `{}` is warming up on Hugging Face — retry \
                             in a moment. Raw: {preview}",
                            self.model,
                        ),
                    }
                }
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

        // Response is a bare nested array: [[f32, ...], ...]
        let vectors: Vec<Vec<f32>> =
            response
                .json()
                .await
                .map_err(|e| LlmError::InvalidResponse {
                    provider: PROVIDER_NAME,
                    message: format!("invalid embedding response: {e}"),
                })?;

        if vectors.len() != input_count {
            return Err(LlmError::InvalidResponse {
                provider: PROVIDER_NAME,
                message: format!(
                    "expected {input_count} embeddings, got {} from model {}",
                    vectors.len(),
                    self.model
                ),
            });
        }
        for vector in &vectors {
            if self.dimension != 0 && vector.len() != self.dimension {
                return Err(LlmError::InvalidResponse {
                    provider: PROVIDER_NAME,
                    message: format!(
                        "expected {} dimensions, got {} from model {}",
                        self.dimension,
                        vector.len(),
                        self.model
                    ),
                });
            }
        }
        Ok(vectors)
    }
}

#[derive(Debug, Serialize)]
struct HfEmbedRequest<'a> {
    inputs: &'a [String],
    normalize: bool,
    truncate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn provider_at(server: &Server, dimension: usize) -> HuggingFaceEmbeddingProvider {
        HuggingFaceEmbeddingProvider::new("hf_test", "BAAI/bge-m3", dimension)
            .expect("provider")
            .with_base_url(server.url())
    }

    const MODEL_PATH: &str = "/models/BAAI/bge-m3/pipeline/feature-extraction";

    #[test]
    fn rejects_empty_api_key() {
        let err = HuggingFaceEmbeddingProvider::new("", "BAAI/bge-m3", 1024)
            .expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn rejects_url_breaking_model_ids() {
        for bad in ["", "a b", "model?x=1", "m#frag", "../../etc", "a..b"] {
            let err = HuggingFaceEmbeddingProvider::new("hf_test", bad, 1024)
                .expect_err("must reject");
            assert_eq!(err.code(), "LLM_INVALID_RESPONSE", "model id `{bad}`");
        }
    }

    #[test]
    fn accepts_org_slash_name_model_ids() {
        let p = HuggingFaceEmbeddingProvider::new(
            "hf_test",
            "sentence-transformers/all-MiniLM-L6-v2",
            384,
        )
        .expect("provider");
        assert_eq!(p.name(), "huggingface");
        assert_eq!(p.model_id(), "sentence-transformers/all-MiniLM-L6-v2");
        assert_eq!(p.dimension(), 384);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_input_returns_empty_output_without_http() {
        let p = HuggingFaceEmbeddingProvider::new("hf_test", "BAAI/bge-m3", 4)
            .expect("provider")
            .with_base_url("http://invalid:1");
        let out = p.embed(Vec::new()).await.expect("empty ok");
        assert!(out.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn embed_posts_inputs_array_and_decodes_nested_array() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", MODEL_PATH)
            .match_header("authorization", "Bearer hf_test")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "inputs": ["hello", "world"],
                "normalize": true,
                "truncate": true
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[[0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]")
            .create_async()
            .await;

        let p = provider_at(&server, 4);
        let vectors = p
            .embed(vec!["hello".into(), "world".into()])
            .await
            .expect("embed");
        assert_eq!(vectors.len(), 2);
        assert!((vectors[1][0] - 1.0).abs() < f32::EPSILON);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn probe_mode_dimension_zero_skips_validation() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", MODEL_PATH)
            .with_status(200)
            .with_body("[[0.5, 0.5, 0.5]]")
            .create_async()
            .await;

        let p = provider_at(&server, 0);
        let vectors = p.embed(vec!["probe".into()]).await.expect("probe ok");
        assert_eq!(vectors[0].len(), 3);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dimension_mismatch_returns_invalid_response() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", MODEL_PATH)
            .with_status(200)
            .with_body("[[0.5, 0.5]]")
            .create_async()
            .await;

        let p = provider_at(&server, 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must reject");
        assert_eq!(err.code(), "LLM_INVALID_RESPONSE");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn count_mismatch_returns_invalid_response() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", MODEL_PATH)
            .with_status(200)
            .with_body("[[0.5, 0.5, 0.5, 0.5]]")
            .create_async()
            .await;

        let p = provider_at(&server, 4);
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
            .mock("POST", MODEL_PATH)
            .with_status(401)
            .with_body("invalid token")
            .create_async()
            .await;

        let p = provider_at(&server, 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must error");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_429_carries_retry_after_seconds() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", MODEL_PATH)
            .with_status(429)
            .with_header("retry-after", "45")
            .with_body("rate limited")
            .create_async()
            .await;

        let p = provider_at(&server, 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must error");
        match err {
            LlmError::RateLimited {
                retry_after_seconds,
                ..
            } => assert_eq!(retry_after_seconds, Some(45)),
            other => panic!("expected RateLimited, got {other:?}"),
        }
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn http_503_loading_maps_to_warm_up_message() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", MODEL_PATH)
            .with_status(503)
            .with_body("{\"error\":\"Model BAAI/bge-m3 is currently loading\",\"estimated_time\":20.0}")
            .create_async()
            .await;

        let p = provider_at(&server, 4);
        let err = p.embed(vec!["x".into()]).await.expect_err("must error");
        assert_eq!(err.code(), "LLM_PROVIDER_UNAVAILABLE");
        assert!(err.to_string().contains("warming up"), "got: {err}");
        mock.assert_async().await;
    }
}
