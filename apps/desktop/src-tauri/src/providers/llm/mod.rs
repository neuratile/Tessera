//! LLM provider abstraction and concrete implementations.
//!
//! Per `rules.md` §5.2 + §12.2 and ADR-0003: services depend on the
//! `LlmProvider` trait, never on a concrete SDK. Swapping providers is
//! a configuration change, not a code change.
//!
//! # Layout
//!
//! - [`error`] — typed [`LlmError`] enum (`rules.md` §5.3).
//! - [`types`] — request / response / streaming chunk types shared by
//!   every concrete implementation.
//! - [`ollama`] — local OpenAI-compatible provider (default; no key).
//! - [`openai`] — `OpenAI` cloud chat completions.
//! - [`openrouter`] — OpenAI-compatible aggregator.
//! - [`anthropic`] — Anthropic `/v1/messages` (different request shape).
//!
//! Streaming uses `Pin<Box<dyn Stream<Item = Result<Chunk, LlmError>> + Send>>`
//! so trait objects (`Arc<dyn LlmProvider>`) work through the factory.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

pub mod error;
pub mod openai_compat;
pub mod types;

pub mod anthropic;
pub mod ollama;
pub mod openai;
pub mod openrouter;

pub use error::LlmError;
pub use types::{
    Chunk, Content, FinishReason, GenerateRequest, GenerateResponse, Message, ProviderCapabilities,
    Role, ToolSchema, Usage,
};

/// Boxed stream type returned by `LlmProvider::stream`. See ADR-0003 for
/// the rationale (trait-object dispatch requires a concrete return type).
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<Chunk, LlmError>> + Send>>;

/// Provider-agnostic LLM interface. All concrete providers (Ollama,
/// `OpenAI`, `OpenRouter`, Anthropic) implement this trait.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable identifier used in logs and `LlmError::provider`. Lowercase
    /// snake-case (`ollama`, `openai`, `openrouter`, `anthropic`).
    fn name(&self) -> &'static str;

    /// Capabilities of the active model / provider combination.
    fn capabilities(&self) -> &ProviderCapabilities;

    /// Approximate token count for budget planning. Heuristic in Phase 2
    /// (see ADR-0003); per-provider tokenizers arrive in Phase 3.
    fn count_tokens(&self, text: &str) -> usize;

    /// Run a non-streaming generation. Default impl drains
    /// [`Self::stream`] so each provider only has to implement the
    /// streaming path. Override for providers whose non-streaming
    /// endpoint is meaningfully cheaper.
    ///
    /// # Errors
    ///
    /// Returns [`LlmError`] for any transport, auth, rate-limit, schema,
    /// or context-window failure.
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, LlmError> {
        use futures::StreamExt;

        let mut stream = self.stream(request);
        let mut content = Vec::new();
        let mut text_buffer = String::new();
        let mut tool_buffers: Vec<(String, String, String)> = Vec::new();
        let mut usage = Usage::default();
        let mut finish_reason = FinishReason::Other;

        while let Some(chunk) = stream.next().await {
            match chunk? {
                Chunk::TextDelta(s) => text_buffer.push_str(&s),
                Chunk::ToolCallStart { id, name } => {
                    if !text_buffer.is_empty() {
                        content.push(Content::Text {
                            text: std::mem::take(&mut text_buffer),
                        });
                    }
                    tool_buffers.push((id, name, String::new()));
                }
                Chunk::ToolCallArgsDelta { id, json_fragment } => {
                    if let Some(slot) = tool_buffers
                        .iter_mut()
                        .find(|(slot_id, _, _)| slot_id == &id)
                    {
                        slot.2.push_str(&json_fragment);
                    } else {
                        // No prior `ToolCallStart` carried this id. Some
                        // OpenAI-compat providers split tool-call name and
                        // arguments across chunks where the name is only
                        // present in the first chunk; if a later chunk
                        // arrives with a different (synthetic) id, the
                        // args would otherwise be silently dropped. Create
                        // a slot with an empty name so the aggregator
                        // still captures the JSON fragment.
                        tracing::warn!(
                            tool_id = %id,
                            "ToolCallArgsDelta arrived without matching ToolCallStart; creating placeholder slot"
                        );
                        tool_buffers.push((id, String::new(), json_fragment));
                    }
                }
                Chunk::Done {
                    usage: u,
                    finish_reason: fr,
                } => {
                    usage = u;
                    finish_reason = fr;
                }
            }
        }

        if !text_buffer.is_empty() {
            content.push(Content::Text { text: text_buffer });
        }
        for (id, name, args) in tool_buffers {
            content.push(Content::ToolUse { id, name, args });
        }

        Ok(GenerateResponse {
            content,
            usage,
            finish_reason,
        })
    }

    /// Stream output incrementally. Final chunk is `Chunk::Done`; no
    /// chunks follow it. Errors mid-stream surface as `Err(LlmError)`
    /// items and end the stream.
    fn stream(&self, request: GenerateRequest) -> ChunkStream;
}

/// Heuristic token counter shared by every provider until per-provider
/// tokenizers land in Phase 3 (ADR-0003 §"Token counting"). Roughly
/// 4 characters per token across English / code; deliberately
/// conservative — overestimates rather than underestimates budgets.
#[must_use]
pub fn approximate_token_count(text: &str) -> usize {
    // Use char count rather than byte count so multi-byte UTF-8 does not
    // inflate the estimate. Round up so empty strings count as 0 but
    // single chars count as 1.
    let chars = text.chars().count();
    chars.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximate_token_count_handles_empty() {
        assert_eq!(approximate_token_count(""), 0);
    }

    #[test]
    fn approximate_token_count_rounds_up() {
        assert_eq!(approximate_token_count("a"), 1);
        assert_eq!(approximate_token_count("abcd"), 1);
        assert_eq!(approximate_token_count("abcde"), 2);
    }

    #[test]
    fn approximate_token_count_handles_multibyte() {
        // Each emoji is one char (2-4 bytes in UTF-8); count must use
        // chars not bytes.
        assert_eq!(approximate_token_count("🦀🦀🦀🦀"), 1);
    }
}
