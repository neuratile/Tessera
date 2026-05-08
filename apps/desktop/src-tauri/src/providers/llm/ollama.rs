//! Ollama LLM provider — local `OpenAI`-compatible chat completions.
//!
//! Talks to `${OLLAMA_BASE_URL}/v1/chat/completions`. No authentication
//! header required (Ollama runs locally, single-user). The wire format,
//! SSE parser, and HTTP-error mapping are shared with the cloud
//! `OpenAI`-compatible providers via [`super::openai_compat`].

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use reqwest::Client;

use super::error::LlmError;
use super::openai_compat;
use super::types::{GenerateRequest, ProviderCapabilities};
use super::{ChunkStream, LlmProvider};
use crate::utils::provider_base_url::normalize_ollama_base_url;

/// Provider name used in `LlmError::provider` and logs.
pub const PROVIDER_NAME: &str = "ollama";

/// Conservative default — long generations against a slow local model
/// must not be cut off. 5 minutes covers cold-start model loads on
/// memory-constrained CI runners (GitHub Actions free tier evicts the
/// previously-loaded model when a second model is loaded, so the next
/// call pays the full reload cost) plus the actual tool-call response.
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

/// Ollama provider. Holds an HTTP client and the resolved base URL.
#[derive(Debug, Clone)]
pub struct OllamaProvider {
    base_url: String,
    client: Client,
    capabilities: ProviderCapabilities,
}

impl OllamaProvider {
    /// Construct a provider pointed at `base_url` (e.g.
    /// `http://localhost:11434`). Trailing slash is optional.
    ///
    /// # Errors
    ///
    /// Returns `LlmError::ProviderUnavailable` if the underlying HTTP
    /// client cannot be built (rare — only happens if the platform
    /// rejects rustls).
    pub fn new(base_url: impl Into<String>) -> Result<Self, LlmError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| LlmError::ProviderUnavailable {
                provider: PROVIDER_NAME,
                message: format!("failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            base_url: normalize_base_url(&base_url.into()),
            client,
            capabilities: ProviderCapabilities {
                supports_tools: true,
                supports_streaming: true,
                // Conservative: Qwen2.5 Coder runs at 32K by default.
                // Larger context windows are available per-model.
                max_context_tokens: 32_768,
                max_output_tokens: 8_192,
            },
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    fn count_tokens(&self, text: &str) -> usize {
        super::approximate_token_count(text)
    }

    fn stream(&self, request: GenerateRequest) -> ChunkStream {
        let body = openai_compat::build_request_payload(&request, true);
        let endpoint = self.endpoint();
        openai_compat::stream_chat_completions(openai_compat::ChatRequest {
            provider: PROVIDER_NAME,
            endpoint: &endpoint,
            headers: HeaderMap::new(),
            body,
            client: &self.client,
        })
    }
}

/// Strip a trailing `/` from `base_url` so endpoint construction never
/// produces double slashes.
fn normalize_base_url(raw: &str) -> String {
    normalize_ollama_base_url(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::llm::types::{Chunk, Content, FinishReason, Message};
    use futures::StreamExt;
    use mockito::Server;

    fn sample_request(model: &str) -> GenerateRequest {
        GenerateRequest {
            model: model.into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        }
    }

    #[test]
    fn normalize_base_url_strips_trailing_slash() {
        assert_eq!(normalize_base_url("http://x:11434/"), "http://x:11434");
        assert_eq!(normalize_base_url("http://x:11434"), "http://x:11434");
    }

    #[test]
    fn endpoint_appends_path() {
        let provider = OllamaProvider::new("http://localhost:11434").expect("provider");
        assert_eq!(
            provider.endpoint(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn capabilities_match_phase_2_defaults() {
        let provider = OllamaProvider::new("http://x").expect("provider");
        let cap = provider.capabilities();
        assert!(cap.supports_streaming);
        assert!(cap.supports_tools);
        assert_eq!(cap.max_context_tokens, 32_768);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_emits_text_then_done_against_mock() {
        let mut server = Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n\
                    data: [DONE]\n\n";
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;

        let provider = OllamaProvider::new(server.url()).expect("provider");
        let mut stream = provider.stream(sample_request("qwen2.5-coder:7b"));
        let mut texts = Vec::new();
        let mut done_seen = false;

        while let Some(chunk) = stream.next().await {
            match chunk.expect("chunk") {
                Chunk::TextDelta(t) => texts.push(t),
                Chunk::Done { .. } => done_seen = true,
                _ => {}
            }
        }

        assert_eq!(texts, vec!["hello".to_string(), " world".to_string()]);
        assert!(done_seen, "must observe Done chunk");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_yields_auth_failed_on_401() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(401)
            .with_body("unauthorized")
            .create_async()
            .await;

        let provider = OllamaProvider::new(server.url()).expect("provider");
        let mut stream = provider.stream(sample_request("m"));
        let first = stream.next().await.expect("at least one yield");
        let err = first.expect_err("expect error item");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_yields_rate_limited_on_429() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(429)
            .with_body("slow down")
            .create_async()
            .await;

        let provider = OllamaProvider::new(server.url()).expect("provider");
        let mut stream = provider.stream(sample_request("m"));
        let first = stream.next().await.expect("yield");
        let err = first.expect_err("error");
        assert_eq!(err.code(), "LLM_RATE_LIMITED");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generate_drains_stream_into_response() {
        let mut server = Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n";
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let provider = OllamaProvider::new(server.url()).expect("provider");
        let response = provider
            .generate(sample_request("m"))
            .await
            .expect("generate");
        assert_eq!(response.usage.input_tokens, 1);
        assert_eq!(response.usage.output_tokens, 1);
        assert_eq!(response.finish_reason, FinishReason::Stop);
        let text = response
            .content
            .iter()
            .filter_map(|c| match c {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(text, "hi");
        mock.assert_async().await;
    }
}
