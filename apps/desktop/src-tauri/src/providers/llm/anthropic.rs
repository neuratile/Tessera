//! Anthropic LLM provider — `Claude` chat via `/v1/messages`.
//!
//! Different wire format from `OpenAI`:
//!
//! - System prompt is a top-level `system` field, not a message with
//!   `role: "system"`.
//! - Streaming uses named SSE events (`event: content_block_delta`)
//!   rather than a single anonymous `data:` channel.
//! - Tool calls are typed content blocks with `type: "tool_use"` and
//!   incremental JSON via `input_json_delta`.
//! - Authentication uses `x-api-key` plus a mandatory
//!   `anthropic-version` header.
//! - `max_tokens` is required on every request.
//!
//! The provider translates these specifics at the wire boundary so
//! service code consumes the same `Chunk` stream as for any other
//! provider.

use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Client;
use serde::Deserialize;

use super::error::LlmError;
use super::openai_compat;
use super::types::{
    Chunk, FinishReason, GenerateRequest, Message, ProviderCapabilities, Role, ToolSchema, Usage,
};
use super::{ChunkStream, LlmProvider};

/// Provider name used in `LlmError::provider` and logs.
pub const PROVIDER_NAME: &str = "anthropic";

/// Default cloud endpoint base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// API version pinned for stable wire-format guarantees.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

/// Default `max_tokens` if caller does not supply one. Anthropic
/// requires the field; `4096` is a sane upper bound for most artifact
/// generations and well under the 8192 ceiling on Sonnet.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic provider.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    base_url: String,
    auth_header: HeaderValue,
    client: Client,
    capabilities: ProviderCapabilities,
}

impl AnthropicProvider {
    /// Construct a provider using the default Anthropic cloud endpoint.
    ///
    /// # Errors
    ///
    /// See [`Self::with_base_url`] — same conditions.
    pub fn new(api_key: &str) -> Result<Self, LlmError> {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    /// Construct a provider pointed at a custom base URL.
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

        let mut auth_value = HeaderValue::from_str(api_key).map_err(|_| LlmError::AuthFailed {
            provider: PROVIDER_NAME,
            message: "API key contains invalid characters for an HTTP header".into(),
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
            client,
            capabilities: ProviderCapabilities {
                supports_tools: true,
                supports_streaming: true,
                // Claude 3.5 Sonnet / Claude 4 family — 200K context,
                // 8K-output ceiling on Sonnet (newer Claude can be
                // higher; refine at the service layer).
                max_context_tokens: 200_000,
                max_output_tokens: 8_192,
            },
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    fn request_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            HeaderName::from_static("x-api-key"),
            self.auth_header.clone(),
        );
        if let Ok(version) = HeaderValue::from_str(ANTHROPIC_VERSION) {
            h.insert(HeaderName::from_static("anthropic-version"), version);
        }
        h
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
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
        let endpoint = self.endpoint();
        let headers = self.request_headers();
        let body = build_anthropic_request(&request, true);
        let client = self.client.clone();

        let s = try_stream! {
            let response = client
                .post(&endpoint)
                .headers(headers)
                .json(&body)
                .send()
                .await
                .map_err(|e| LlmError::from_reqwest(PROVIDER_NAME, &e))?;

            let status = response.status();
            let response_headers = response.headers().clone();
            let body_stream = if status.is_success() {
                response.bytes_stream()
            } else {
                let text = response.text().await.unwrap_or_default();
                Err(openai_compat::map_http_error(
                    PROVIDER_NAME,
                    status,
                    &response_headers,
                    &text,
                ))?;
                unreachable!("yielded error above")
            };

            let mut byte_stream = body_stream;
            let mut buffer = String::new();
            let mut state = AnthropicStreamState::default();

            while let Some(bytes) = byte_stream.next().await {
                let bytes = bytes.map_err(|e| LlmError::StreamInterrupted {
                    provider: PROVIDER_NAME,
                    message: e.to_string(),
                })?;
                let text = std::str::from_utf8(&bytes).map_err(|e| LlmError::StreamInterrupted {
                    provider: PROVIDER_NAME,
                    message: format!("non-utf8 stream bytes: {e}"),
                })?;
                buffer.push_str(text);

                while let Some((event, rest)) = split_sse_event(&buffer) {
                    let event_owned = event.to_string();
                    buffer = rest.to_string();
                    for chunk in parse_sse_event(&event_owned, &mut state)? {
                        yield chunk;
                    }
                }
            }

            // Drain any complete-but-unterminated event left in the buffer
            // when the server closes the connection without a final
            // `\n\n`. Without this the last `message_stop` (and thus the
            // `Chunk::Done`) can be silently dropped.
            let trailing = buffer.trim();
            if !trailing.is_empty() {
                for chunk in parse_sse_event(trailing, &mut state)? {
                    yield chunk;
                }
            }
        };

        Box::pin(s)
    }
}

/// Build the JSON body sent to `/v1/messages`. System prompts are
/// extracted to the top-level `system` field per Anthropic's schema.
fn build_anthropic_request(req: &GenerateRequest, stream: bool) -> serde_json::Value {
    let (system_text, messages) = split_system_messages(&req.messages);

    let mut payload = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": stream,
    });

    if !system_text.is_empty() {
        payload["system"] = serde_json::json!(system_text);
    }
    if !req.tools.is_empty() {
        payload["tools"] = req.tools.iter().map(tool_to_anthropic).collect();
    }
    if let Some(t) = req.temperature {
        payload["temperature"] = serde_json::json!(t);
    }
    if !req.stop_sequences.is_empty() {
        payload["stop_sequences"] = serde_json::json!(req.stop_sequences);
    }
    payload
}

/// Pull every `Role::System` message out of `messages` (concatenated
/// into one system string) and return the remainder as Anthropic-style
/// message objects.
fn split_system_messages(messages: &[Message]) -> (String, Vec<serde_json::Value>) {
    let mut system_parts = Vec::new();
    let mut out = Vec::new();

    for msg in messages {
        if msg.role == Role::System {
            for c in &msg.content {
                if let super::types::Content::Text { text } = c {
                    system_parts.push(text.clone());
                }
            }
            continue;
        }
        out.push(message_to_anthropic(msg));
    }

    (system_parts.join("\n\n"), out)
}

fn message_to_anthropic(msg: &Message) -> serde_json::Value {
    // System messages should already have been filtered upstream by
    // split_system_messages; if any slipped through, fall back to
    // user. Tool results are represented as user-role messages with a
    // tool_result content block in Anthropic's schema.
    let role = match msg.role {
        Role::Assistant => "assistant",
        Role::User | Role::Tool | Role::System => "user",
    };

    let mut content_blocks = Vec::new();
    for c in &msg.content {
        match c {
            super::types::Content::Text { text } => {
                content_blocks.push(serde_json::json!({"type": "text", "text": text}));
            }
            super::types::Content::ToolUse { id, name, args } => {
                // If the previously-captured streaming args do not parse,
                // fall back to an empty object rather than `null`. Many
                // Anthropic tool schemas reject `null` for `input` and
                // would 400 the whole follow-up call; an empty object
                // lets the model see the tool was invoked and re-issue
                // arguments in the next turn.
                let parsed_args: serde_json::Value = serde_json::from_str(args)
                    .unwrap_or_else(|err| {
                        tracing::warn!(
                            tool_id = %id,
                            tool_name = %name,
                            error = %err,
                            "tool_use args failed to parse as JSON; substituting empty object"
                        );
                        serde_json::json!({})
                    });
                content_blocks.push(serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": parsed_args,
                }));
            }
            super::types::Content::ToolResult { id, content } => {
                content_blocks.push(serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": content,
                }));
            }
        }
    }

    serde_json::json!({"role": role, "content": content_blocks})
}

fn tool_to_anthropic(tool: &ToolSchema) -> serde_json::Value {
    serde_json::json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.parameters_schema,
    })
}

/// Carries cross-event state for the SSE parser. Anthropic delivers
/// tool-call IDs / names in `content_block_start` and the JSON
/// fragments in subsequent `input_json_delta` events; we map both
/// onto the index that ties them together.
#[derive(Debug, Default)]
struct AnthropicStreamState {
    /// `content_block.index` -> tool-call id assigned at start.
    tool_ids: std::collections::HashMap<u32, String>,
    usage: Usage,
    finish_reason: Option<FinishReason>,
}

fn split_sse_event(buffer: &str) -> Option<(&str, &str)> {
    if let Some(idx) = buffer.find("\n\n") {
        let (event, rest) = buffer.split_at(idx);
        Some((event, &rest[2..]))
    } else if let Some(idx) = buffer.find("\r\n\r\n") {
        let (event, rest) = buffer.split_at(idx);
        Some((event, &rest[4..]))
    } else {
        None
    }
}

fn parse_sse_event(raw: &str, state: &mut AnthropicStreamState) -> Result<Vec<Chunk>, LlmError> {
    // SSE event = optional `event: <name>` line + one or more
    // `data: <payload>` lines. Anthropic always sends both.
    let mut data_lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if let Some(payload) = line.strip_prefix("data:") {
            data_lines.push(payload.trim());
        }
        // We do not depend on `event:` — the type discriminator lives
        // inside the JSON payload anyway.
    }

    if data_lines.is_empty() {
        return Ok(Vec::new());
    }

    let payload = data_lines.join("");
    let parsed: AnthropicEvent =
        serde_json::from_str(&payload).map_err(|e| LlmError::InvalidResponse {
            provider: PROVIDER_NAME,
            message: format!("invalid stream JSON: {e}"),
        })?;

    Ok(translate_event(parsed, state))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicEvent {
    MessageStart {
        #[serde(default)]
        message: AnthropicMessageStart,
    },
    ContentBlockStart {
        index: u32,
        content_block: AnthropicContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: AnthropicDelta,
    },
    ContentBlockStop,
    MessageDelta {
        #[serde(default)]
        delta: AnthropicMessageDelta,
        #[serde(default)]
        usage: Option<AnthropicUsage>,
    },
    MessageStop,
    Ping,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicMessageStart {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicMessageDelta {
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text,
    ToolUse {
        id: String,
        name: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

fn translate_event(event: AnthropicEvent, state: &mut AnthropicStreamState) -> Vec<Chunk> {
    let mut out = Vec::new();
    match event {
        AnthropicEvent::MessageStart { message } => {
            if let Some(u) = message.usage {
                state.usage.input_tokens = u.input_tokens;
            }
        }
        AnthropicEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            if let AnthropicContentBlock::ToolUse { id, name } = content_block {
                state.tool_ids.insert(index, id.clone());
                out.push(Chunk::ToolCallStart { id, name });
            }
        }
        AnthropicEvent::ContentBlockDelta { index, delta } => match delta {
            AnthropicDelta::TextDelta { text } => {
                if !text.is_empty() {
                    out.push(Chunk::TextDelta(text));
                }
            }
            AnthropicDelta::InputJsonDelta { partial_json } => {
                if let Some(id) = state.tool_ids.get(&index) {
                    if !partial_json.is_empty() {
                        out.push(Chunk::ToolCallArgsDelta {
                            id: id.clone(),
                            json_fragment: partial_json,
                        });
                    }
                } else if !partial_json.is_empty() {
                    tracing::warn!(
                        index = index,
                        "InputJsonDelta arrived for unknown content_block index; synthesizing tool id"
                    );
                    // Synthesize an id so downstream aggregators can still
                    // collect the fragment instead of silently dropping
                    // it. The aggregator buffers by id, so a stable
                    // per-index synthetic id keeps subsequent deltas in
                    // the same bucket.
                    let synthetic_id = format!("orphan_tool_{index}");
                    state.tool_ids.insert(index, synthetic_id.clone());
                    out.push(Chunk::ToolCallStart {
                        id: synthetic_id.clone(),
                        name: String::new(),
                    });
                    out.push(Chunk::ToolCallArgsDelta {
                        id: synthetic_id,
                        json_fragment: partial_json,
                    });
                }
            }
            AnthropicDelta::Unknown => {}
        },
        AnthropicEvent::MessageDelta { delta, usage } => {
            if let Some(u) = usage {
                if u.output_tokens > 0 {
                    state.usage.output_tokens = u.output_tokens;
                }
            }
            if let Some(reason) = delta.stop_reason {
                state.finish_reason = Some(parse_anthropic_stop_reason(&reason));
            }
        }
        AnthropicEvent::MessageStop => {
            out.push(Chunk::Done {
                usage: state.usage,
                finish_reason: state.finish_reason.unwrap_or(FinishReason::Stop),
            });
        }
        AnthropicEvent::ContentBlockStop | AnthropicEvent::Ping | AnthropicEvent::Unknown => {}
    }
    out
}

fn parse_anthropic_stop_reason(raw: &str) -> FinishReason {
    match raw {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::MaxTokens,
        "tool_use" => FinishReason::ToolUse,
        "stop_sequence" => FinishReason::StopSequence,
        _ => FinishReason::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::llm::types::{Chunk, Content};
    use futures::StreamExt;
    use mockito::Server;

    fn sample_request() -> GenerateRequest {
        GenerateRequest {
            model: "claude-3-5-sonnet-20241022".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(64),
            stop_sequences: Vec::new(),
        }
    }

    #[test]
    fn empty_api_key_is_rejected() {
        let err = AnthropicProvider::new("").expect_err("must reject");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
    }

    #[test]
    fn capabilities_advertise_anthropic_floor() {
        let provider = AnthropicProvider::new("sk-ant-test").expect("provider");
        let cap = provider.capabilities();
        assert!(cap.supports_tools);
        assert!(cap.supports_streaming);
        assert_eq!(cap.max_context_tokens, 200_000);
    }

    #[test]
    fn split_system_messages_extracts_text() {
        let msgs = vec![
            Message::system("be brief"),
            Message::user("hi"),
            Message::system("also be helpful"),
            Message::assistant("ok"),
        ];
        let (system, rest) = split_system_messages(&msgs);
        assert_eq!(system, "be brief\n\nalso be helpful");
        assert_eq!(rest.len(), 2);
        assert_eq!(rest[0]["role"], "user");
        assert_eq!(rest[1]["role"], "assistant");
    }

    #[test]
    fn build_request_sets_top_level_system() {
        let req = GenerateRequest {
            model: "m".into(),
            messages: vec![Message::system("be brief"), Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(100),
            stop_sequences: Vec::new(),
        };
        let body = build_anthropic_request(&req, true);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["max_tokens"], 100);
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"].as_array().expect("array").len(), 1);
    }

    #[test]
    fn build_request_uses_default_max_tokens() {
        let req = GenerateRequest {
            model: "m".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        };
        let body = build_anthropic_request(&req, false);
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn message_to_anthropic_emits_typed_blocks() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                Content::text("hello"),
                Content::ToolUse {
                    id: "tool_1".into(),
                    name: "search".into(),
                    args: r#"{"q":"x"}"#.into(),
                },
            ],
        };
        let json = message_to_anthropic(&msg);
        assert_eq!(json["role"], "assistant");
        let blocks = json["content"].as_array().expect("blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "tool_1");
        assert_eq!(blocks[1]["input"]["q"], "x");
    }

    #[test]
    fn tool_role_maps_to_user_with_tool_result_block() {
        let msg = Message {
            role: Role::Tool,
            content: vec![Content::ToolResult {
                id: "tool_1".into(),
                content: "{\"ok\":true}".into(),
            }],
        };
        let json = message_to_anthropic(&msg);
        assert_eq!(json["role"], "user");
        let blocks = json["content"].as_array().expect("blocks");
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "tool_1");
    }

    #[test]
    fn parse_text_delta_event() {
        let raw =
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        let mut state = AnthropicStreamState::default();
        let chunks = parse_sse_event(raw, &mut state).expect("parse");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], Chunk::TextDelta("Hello".into()));
    }

    #[test]
    fn parse_message_stop_emits_done() {
        let mut state = AnthropicStreamState {
            tool_ids: std::collections::HashMap::new(),
            usage: Usage {
                input_tokens: 12,
                output_tokens: 7,
            },
            finish_reason: Some(FinishReason::Stop),
        };
        let raw = "data: {\"type\":\"message_stop\"}";
        let chunks = parse_sse_event(raw, &mut state).expect("parse");
        assert_eq!(chunks.len(), 1);
        match chunks[0] {
            Chunk::Done {
                usage,
                finish_reason,
            } => {
                assert_eq!(usage.input_tokens, 12);
                assert_eq!(usage.output_tokens, 7);
                assert_eq!(finish_reason, FinishReason::Stop);
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn parse_ping_event_yields_no_chunks() {
        let mut state = AnthropicStreamState::default();
        let chunks = parse_sse_event("data: {\"type\":\"ping\"}", &mut state).expect("parse");
        assert!(chunks.is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let mut state = AnthropicStreamState::default();
        let err = parse_sse_event("data: {not json", &mut state).expect_err("must fail");
        assert_eq!(err.code(), "LLM_INVALID_RESPONSE");
    }

    #[test]
    fn parse_anthropic_stop_reason_maps_known_values() {
        assert_eq!(parse_anthropic_stop_reason("end_turn"), FinishReason::Stop);
        assert_eq!(
            parse_anthropic_stop_reason("max_tokens"),
            FinishReason::MaxTokens
        );
        assert_eq!(
            parse_anthropic_stop_reason("tool_use"),
            FinishReason::ToolUse
        );
        assert_eq!(
            parse_anthropic_stop_reason("stop_sequence"),
            FinishReason::StopSequence
        );
        assert_eq!(parse_anthropic_stop_reason("?"), FinishReason::Other);
    }

    #[test]
    fn parse_tool_use_block_start_then_args_delta() {
        let mut state = AnthropicStreamState::default();
        let start = "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t_42\",\"name\":\"search\"}}";
        let chunks = parse_sse_event(start, &mut state).expect("parse start");
        assert!(matches!(chunks[0], Chunk::ToolCallStart { .. }));

        let delta = "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\"}}";
        let chunks = parse_sse_event(delta, &mut state).expect("parse delta");
        match &chunks[0] {
            Chunk::ToolCallArgsDelta { id, json_fragment } => {
                assert_eq!(id, "t_42");
                assert_eq!(json_fragment, "{\"q\":");
            }
            other => panic!("expected ToolCallArgsDelta, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_against_mock_yields_text_then_done() {
        let mut server = Server::new_async().await;
        let body = "event: message_start\n\
                    data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"x\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n\
                    event: content_block_start\n\
                    data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
                    event: content_block_delta\n\
                    data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n\
                    event: content_block_delta\n\
                    data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n\
                    event: content_block_stop\n\
                    data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
                    event: message_delta\n\
                    data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":2}}\n\n\
                    event: message_stop\n\
                    data: {\"type\":\"message_stop\"}\n\n";
        let mock = server
            .mock("POST", "/v1/messages")
            .match_header("x-api-key", "sk-ant-123")
            .match_header("anthropic-version", ANTHROPIC_VERSION)
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;

        let provider =
            AnthropicProvider::with_base_url("sk-ant-123", server.url()).expect("provider");
        let mut stream = provider.stream(sample_request());
        let mut texts = Vec::new();
        let mut done_seen: Option<(Usage, FinishReason)> = None;
        while let Some(chunk) = stream.next().await {
            match chunk.expect("chunk") {
                Chunk::TextDelta(t) => texts.push(t),
                Chunk::Done {
                    usage,
                    finish_reason,
                } => done_seen = Some((usage, finish_reason)),
                _ => {}
            }
        }
        assert_eq!(texts, vec!["hello".to_string(), " world".to_string()]);
        let (usage, reason) = done_seen.expect("done observed");
        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 2);
        assert_eq!(reason, FinishReason::Stop);
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stream_handles_401() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/messages")
            .with_status(401)
            .with_body("auth required")
            .create_async()
            .await;

        let provider = AnthropicProvider::with_base_url("k", server.url()).expect("provider");
        let mut stream = provider.stream(sample_request());
        let first = stream.next().await.expect("yield");
        let err = first.expect_err("must error");
        assert_eq!(err.code(), "LLM_AUTH_FAILED");
        mock.assert_async().await;
    }
}
