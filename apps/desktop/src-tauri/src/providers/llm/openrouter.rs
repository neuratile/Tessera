//! `OpenRouter` LLM provider — `OpenAI`-compatible aggregator.
//!
//! Talks to `https://openrouter.ai/api/v1/chat/completions`. Same wire
//! format as `OpenAI`, plus two optional analytics headers
//! (`HTTP-Referer`, `X-Title`) that `OpenRouter` uses to attribute
//! traffic to apps. Both default to identifying this app and are safe
//! to send.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::Client;

use super::error::LlmError;
use super::openai_compat;
use super::types::{GenerateRequest, ProviderCapabilities};
use super::{ChunkStream, LlmProvider};

/// Provider name used in `LlmError::provider` and logs.
pub const PROVIDER_NAME: &str = "openrouter";

/// Default cloud endpoint base URL.
pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api";

/// Default attribution headers — identify this application to
/// `OpenRouter` for ranking on the public app leaderboard. No secrets
/// here; safe to ship.
pub const DEFAULT_REFERER: &str = "https://github.com/Rajveerx11/Tessera";
// Plain ASCII only — `reqwest::HeaderValue::from_str` rejects
// non-ASCII bytes per RFC 9110, so the em-dash glyph cannot land in
// the `x-title` attribution header. An ASCII hyphen carries the
// same meaning to the OpenRouter leaderboard parser.
pub const DEFAULT_TITLE: &str = "Tessera - AI Testing IDE";

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

/// `OpenRouter` provider.
#[derive(Debug, Clone)]
pub struct OpenRouterProvider {
    base_url: String,
    auth_header: HeaderValue,
    referer: String,
    title: String,
    client: Client,
    capabilities: ProviderCapabilities,
}

impl OpenRouterProvider {
    /// Construct a provider using the default `OpenRouter` endpoint and
    /// default attribution headers.
    ///
    /// # Errors
    ///
    /// See [`Self::with_options`] — same conditions.
    pub fn new(api_key: &str) -> Result<Self, LlmError> {
        Self::with_options(
            api_key,
            DEFAULT_BASE_URL,
            DEFAULT_REFERER.to_string(),
            DEFAULT_TITLE.to_string(),
        )
    }

    /// Construct a provider with a custom base URL while keeping the default
    /// attribution headers.
    ///
    /// # Errors
    ///
    /// See [`Self::with_options`] — same conditions.
    pub fn with_base_url(api_key: &str, base_url: impl Into<String>) -> Result<Self, LlmError> {
        Self::with_options(
            api_key,
            base_url,
            DEFAULT_REFERER.to_string(),
            DEFAULT_TITLE.to_string(),
        )
    }

    /// Construct a provider with custom base URL and attribution
    /// headers. Useful for testing and for users who want to override
    /// the public leaderboard attribution.
    ///
    /// # Errors
    ///
    /// Returns `LlmError::AuthFailed` if `api_key` is empty. Returns
    /// `LlmError::ProviderUnavailable` if the HTTP client cannot be
    /// built.
    pub fn with_options(
        api_key: &str,
        base_url: impl Into<String>,
        referer: String,
        title: String,
    ) -> Result<Self, LlmError> {
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
        let mut auth_value = HeaderValue::from_bytes(&header_bytes).map_err(|_| {
            LlmError::AuthFailed {
                provider: PROVIDER_NAME,
                message: "API key contains invalid characters for an HTTP header".into(),
            }
        })?;
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
            referer,
            title,
            client,
            capabilities: ProviderCapabilities {
                supports_tools: true,
                supports_streaming: true,
                // OpenRouter routes to many models; expose the largest
                // common context floor. Service layer can refine per
                // chosen model.
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
        // Attribution headers — best-effort; ignore if invalid (cannot
        // happen with our defaults; ASCII-only validation upstream).
        if let Ok(val) = HeaderValue::from_str(&self.referer) {
            h.insert(HeaderName::from_static("http-referer"), val);
        }
        if let Ok(val) = HeaderValue::from_str(&self.title) {
            h.insert(HeaderName::from_static("x-title"), val);
        }
        h
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
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
            model: "qwen/qwen2.5-coder-32b-instruct".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        }
    }

    #[test]
    fn empty_api_key_is_rejected() {
        let err = OpenRouterProvider::new("").expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn capabilities_match_phase_2_defaults() {
        let provider = OpenRouterProvider::new("sk-or-test").expect("provider");
        let cap = provider.capabilities();
        assert!(cap.supports_tools);
        assert!(cap.supports_streaming);
        assert_eq!(cap.max_context_tokens, 128_000);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_sends_attribution_headers() {
        let mut server = Server::new_async().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("authorization", "Bearer sk-or-123")
            .match_header("http-referer", DEFAULT_REFERER)
            .match_header("x-title", DEFAULT_TITLE)
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let provider = OpenRouterProvider::with_options(
            "sk-or-123",
            server.url(),
            DEFAULT_REFERER.into(),
            DEFAULT_TITLE.into(),
        )
        .expect("provider");
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
    async fn stream_handles_403_as_auth_failed() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(403)
            .with_body("forbidden")
            .create_async()
            .await;

        let provider =
            OpenRouterProvider::with_options("k", server.url(), String::new(), String::new())
                .expect("provider");
        let mut stream = provider.stream(sample_request());
        let first = stream.next().await.expect("yield");
        let err = first.expect_err("expect error");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        mock.assert_async().await;
    }
}
