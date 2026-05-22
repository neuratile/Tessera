//! `OpenAI` cloud LLM provider.
//!
//! Talks to `https://api.openai.com/v1/chat/completions` (or any
//! configured override — useful for Azure `OpenAI` and `OpenAI`-compatible
//! enterprise gateways). Wire format identical to Ollama, so this
//! provider is a thin wrapper over [`super::openai_compat`] that adds
//! the `Authorization: Bearer <key>` header.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;

use super::error::LlmError;
use super::openai_compat;
use super::types::{GenerateRequest, ProviderCapabilities};
use super::{ChunkStream, LlmProvider};

/// Provider name used in `LlmError::provider` and logs.
pub const PROVIDER_NAME: &str = "openai";

/// Default cloud endpoint base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com";

/// Conservative HTTP timeout for cloud calls.
const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

/// `OpenAI` provider.
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    base_url: String,
    auth_header: HeaderValue,
    client: Client,
    capabilities: ProviderCapabilities,
}

impl OpenAiProvider {
    /// Construct a provider using the default `OpenAI` cloud endpoint.
    ///
    /// # Errors
    ///
    /// See [`Self::with_base_url`] — same conditions.
    pub fn new(api_key: &str) -> Result<Self, LlmError> {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    /// Construct a provider pointed at a custom base URL (Azure
    /// `OpenAI`, an enterprise proxy, etc.).
    ///
    /// # Errors
    ///
    /// Returns `LlmError::AuthFailed` if `api_key` is empty (caught
    /// before any request leaves the process — no need to round-trip
    /// the wire). Returns `LlmError::ProviderUnavailable` if the
    /// underlying HTTP client cannot be built.
    pub fn with_base_url(api_key: &str, base_url: impl Into<String>) -> Result<Self, LlmError> {
        if api_key.trim().is_empty() {
            return Err(LlmError::AuthFailed {
                provider: PROVIDER_NAME,
                message: "API key is empty".into(),
            });
        }

        // Build the header value from raw bytes rather than via
        // `format!("Bearer {api_key}")` so the key never lands in a
        // formatter buffer. `set_sensitive(true)` is called before the
        // value can be observed by any logging path.
        let mut header_bytes = Vec::with_capacity(7 + api_key.len());
        header_bytes.extend_from_slice(b"Bearer ");
        header_bytes.extend_from_slice(api_key.as_bytes());
        let mut auth_value = HeaderValue::from_bytes(&header_bytes).map_err(|_| {
            LlmError::AuthFailed {
                provider: PROVIDER_NAME,
                message: "API key contains invalid characters for an HTTP header".into(),
            }
        })?;
        // Always mark the header sensitive so HTTP debug logs do not
        // leak the key (rules.md §9 — never log secrets).
        auth_value.set_sensitive(true);

        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .build()
            .map_err(|e| LlmError::ProviderUnavailable {
                provider: PROVIDER_NAME,
                message: format!("failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth_header: auth_value,
            client,
            capabilities: ProviderCapabilities {
                supports_tools: true,
                supports_streaming: true,
                // GPT-4o family — 128K context, 16K output. Conservative
                // floor; bigger models override at the service layer.
                max_context_tokens: 128_000,
                max_output_tokens: 16_384,
            },
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, self.auth_header.clone());
        h
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
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
            headers: self.auth_headers(),
            body,
            client: &self.client,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::llm::types::{Chunk, Message};
    use futures::StreamExt;
    use mockito::Server;

    fn sample_request() -> GenerateRequest {
        GenerateRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        }
    }

    #[test]
    fn empty_api_key_is_rejected_at_construction() {
        let err = OpenAiProvider::new("").expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn whitespace_api_key_is_rejected_at_construction() {
        let err = OpenAiProvider::new("   ").expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn capabilities_advertise_tools_and_streaming() {
        let provider = OpenAiProvider::new("sk-test-key").expect("provider");
        let cap = provider.capabilities();
        assert!(cap.supports_tools);
        assert!(cap.supports_streaming);
        assert_eq!(cap.max_context_tokens, 128_000);
    }

    #[test]
    fn auth_header_is_marked_sensitive() {
        let provider = OpenAiProvider::new("sk-test-key").expect("provider");
        assert!(provider.auth_header.is_sensitive());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_sends_authorization_header() {
        let mut server = Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("authorization", "Bearer sk-test-123")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let provider =
            OpenAiProvider::with_base_url("sk-test-123", server.url()).expect("provider");
        let mut stream = provider.stream(sample_request());
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Chunk::TextDelta(t) = chunk.expect("chunk") {
                text.push_str(&t);
            }
        }
        assert_eq!(text, "ok");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_propagates_429_with_retry_after() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(429)
            .with_header("retry-after", "30")
            .with_body("slow down")
            .create_async()
            .await;

        let provider = OpenAiProvider::with_base_url("k", server.url()).expect("provider");
        let mut stream = provider.stream(sample_request());
        let first = stream.next().await.expect("yield");
        let err = first.expect_err("expect error");
        match err {
            LlmError::RateLimited {
                retry_after_seconds: Some(s),
                ..
            } => assert_eq!(s, 30),
            other => panic!("expected RateLimited, got {other:?}"),
        }
        mock.assert_async().await;
    }
}
