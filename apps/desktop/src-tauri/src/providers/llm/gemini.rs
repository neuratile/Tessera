//! Google Gemini LLM provider — Gemini API via its OpenAI-compatible
//! surface.
//!
//! Google AI Studio exposes an OpenAI-compatible endpoint at
//! `https://generativelanguage.googleapis.com/v1beta/openai/` that
//! accepts the standard chat-completions wire format, including SSE
//! streaming and function calling. Authentication uses a plain
//! `Authorization: Bearer <key>` header (the native Gemini REST API
//! uses `x-goog-api-key`, but the compatibility layer takes Bearer).
//!
//! Reusing the compatibility surface keeps this provider a thin
//! wrapper over [`openai_compat`], exactly like `OpenRouter` — no
//! Gemini-specific stream parser to maintain.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;

use super::error::LlmError;
use super::openai_compat;
use super::types::{GenerateRequest, ProviderCapabilities};
use super::{ChunkStream, LlmProvider, StreamPolicy};

/// Provider name used in `LlmError::provider` and logs.
pub const PROVIDER_NAME: &str = "gemini";

/// Default cloud endpoint base URL (host root; the OpenAI-compatible
/// path segment is appended by [`GeminiProvider::endpoint`]).
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

/// Path of the OpenAI-compatible surface relative to the base URL.
pub const OPENAI_COMPAT_PATH: &str = "/v1beta/openai";

/// Cap on establishing the TCP+TLS connection — fast-fails an
/// unreachable endpoint without consuming the read budget.
const CONNECT_TIMEOUT_SECONDS: u64 = 30;

/// Idle/read timeout for the streaming response: the maximum gap
/// allowed *between* SSE chunks, not a deadline on the whole request. A
/// total `.timeout()` would abort long but healthy generations; an idle
/// timeout only fires when the provider goes silent.
const STREAM_READ_TIMEOUT_SECONDS: u64 = 120;

/// Google Gemini provider.
#[derive(Debug, Clone)]
pub struct GeminiProvider {
    base_url: String,
    auth_header: HeaderValue,
    client: Client,
    capabilities: ProviderCapabilities,
}

impl GeminiProvider {
    /// Construct a provider using the default Google AI Studio endpoint.
    ///
    /// # Errors
    ///
    /// See [`Self::with_base_url`] — same conditions.
    pub fn new(api_key: &str) -> Result<Self, LlmError> {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    /// Construct a provider pointed at a custom base URL (host root —
    /// `/v1beta/openai` is appended internally).
    ///
    /// # Errors
    ///
    /// Returns `LlmError::AuthFailed` for empty / whitespace / invalid
    /// API keys. Returns `LlmError::ProviderUnavailable` if the HTTP
    /// client cannot be built.
    pub fn with_base_url(api_key: &str, base_url: impl Into<String>) -> Result<Self, LlmError> {
        if api_key.trim().is_empty() {
            return Err(LlmError::AuthFailed {
                provider: PROVIDER_NAME,
                message: "API key is empty".into(),
            });
        }

        // Build the header from raw bytes rather than via `format!` so
        // the API key never traverses a formatter buffer. Marked
        // sensitive immediately so any HTTP debug logging redacts it.
        let mut header_bytes = Vec::with_capacity(7 + api_key.len());
        header_bytes.extend_from_slice(b"Bearer ");
        header_bytes.extend_from_slice(api_key.as_bytes());
        let mut auth_value =
            HeaderValue::from_bytes(&header_bytes).map_err(|_| LlmError::AuthFailed {
                provider: PROVIDER_NAME,
                message: "API key contains invalid characters for an HTTP header".into(),
            })?;
        auth_value.set_sensitive(true);

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECONDS))
            .read_timeout(Duration::from_secs(STREAM_READ_TIMEOUT_SECONDS))
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
                // Gemini 2.x / 2.5 family — 1M-token context window;
                // 65K-output ceiling on 2.5 models. Service layer can
                // refine per chosen model.
                max_context_tokens: 1_048_576,
                max_output_tokens: 65_536,
            },
        })
    }

    fn endpoint(&self) -> String {
        format!("{}{OPENAI_COMPAT_PATH}/chat/completions", self.base_url)
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, self.auth_header.clone());
        h
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
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
        self.stream_with_policy(request, StreamPolicy::default())
    }

    fn stream_with_policy(&self, request: GenerateRequest, policy: StreamPolicy) -> ChunkStream {
        let body = openai_compat::build_request_payload(&request, true);
        let endpoint = self.endpoint();
        openai_compat::stream_chat_completions(openai_compat::ChatRequest {
            provider: PROVIDER_NAME,
            endpoint: &endpoint,
            headers: self.auth_headers(),
            body,
            client: &self.client,
            policy,
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
            model: "gemini-2.5-flash".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        }
    }

    #[test]
    fn empty_api_key_is_rejected() {
        let err = GeminiProvider::new("").expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn whitespace_api_key_is_rejected() {
        let err = GeminiProvider::new("   ").expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn capabilities_advertise_gemini_window() {
        let provider = GeminiProvider::new("AIza-test").expect("provider");
        let cap = provider.capabilities();
        assert!(cap.supports_tools);
        assert!(cap.supports_streaming);
        assert_eq!(cap.max_context_tokens, 1_048_576);
        assert_eq!(cap.max_output_tokens, 65_536);
    }

    #[test]
    fn endpoint_appends_openai_compat_path() {
        let provider =
            GeminiProvider::with_base_url("AIza-test", "https://example.test/").expect("provider");
        assert_eq!(
            provider.endpoint(),
            "https://example.test/v1beta/openai/chat/completions"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_sends_bearer_auth_to_compat_endpoint() {
        let mut server = Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let mock = server
            .mock("POST", "/v1beta/openai/chat/completions")
            .match_header("authorization", "Bearer AIza-123")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let provider = GeminiProvider::with_base_url("AIza-123", server.url()).expect("provider");
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
    async fn stream_handles_401_as_auth_failed() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1beta/openai/chat/completions")
            .with_status(401)
            .with_body("API key not valid")
            .create_async()
            .await;

        let provider = GeminiProvider::with_base_url("k", server.url()).expect("provider");
        let mut stream = provider.stream(sample_request());
        let first = stream.next().await.expect("yield");
        let err = first.expect_err("must error");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        mock.assert_async().await;
    }
}
