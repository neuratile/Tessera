//! Shared building blocks for `OpenAI`-compatible providers.
//!
//! Three providers in Phase 2 share the same wire format: Ollama (no
//! auth), `OpenAI` (Bearer auth on `api.openai.com`), and `OpenRouter`
//! (Bearer auth on `openrouter.ai`, plus optional analytics headers).
//! Rather than three near-identical SSE parsers, this module exposes:
//!
//! - [`build_request_payload`] — construct the JSON body sent to
//!   `/v1/chat/completions`.
//! - [`stream_chat_completions`] — issue the POST, parse the SSE
//!   response into `Chunk`s, and emit them through a boxed stream.
//! - [`map_http_error`] — uniform HTTP-status → `LlmError` mapping.
//!
//! Concrete providers (ollama, openai, openrouter) become thin wrappers
//! that supply config (URL, headers, provider name) and forward to
//! these helpers.

use async_stream::try_stream;
use futures::stream::StreamExt;
use reqwest::header::HeaderMap;
use reqwest::Client;
use serde::Deserialize;

use super::error::LlmError;
use super::types::{Chunk, FinishReason, GenerateRequest, Message, Role, ToolSchema, Usage};
use super::ChunkStream;

/// Configuration handed to [`stream_chat_completions`] by each
/// concrete provider. `extra_headers` covers things like `OpenRouter`'s
/// `HTTP-Referer` / `X-Title`.
pub struct ChatRequest<'a> {
    pub provider: &'static str,
    pub endpoint: &'a str,
    pub headers: HeaderMap,
    pub body: serde_json::Value,
    pub client: &'a Client,
}

/// Issue the POST and yield `Chunk`s as they arrive. Error responses
/// are translated via [`map_http_error`] and surfaced as the first
/// (and only) item on the stream.
#[must_use]
pub fn stream_chat_completions(req: ChatRequest<'_>) -> ChunkStream {
    let provider = req.provider;
    let endpoint = req.endpoint.to_string();
    let headers = req.headers;
    let body = req.body;
    let client = req.client.clone();

    let s = try_stream! {
        let response = client
            .post(&endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::from_reqwest(provider, &e))?;

        let status = response.status();
        let headers_in = response.headers().clone();
        let body_stream = if status.is_success() {
            response.bytes_stream()
        } else {
            let text = response.text().await.unwrap_or_default();
            Err(map_http_error(provider, status, &headers_in, &text))?;
            // Unreachable: `Err(...)?` propagates above. Compiler can't
            // see that, so we feed it a value that satisfies the type.
            unreachable!("yielded error above")
        };

        let mut byte_stream = body_stream;
        let mut buffer = String::new();

        while let Some(bytes) = byte_stream.next().await {
            let bytes = bytes.map_err(|e| LlmError::StreamInterrupted {
                provider,
                message: e.to_string(),
            })?;

            let text = std::str::from_utf8(&bytes).map_err(|e| LlmError::StreamInterrupted {
                provider,
                message: format!("non-utf8 stream bytes: {e}"),
            })?;
            buffer.push_str(text);

            while let Some((event, rest)) = split_sse_event(&buffer) {
                let event_owned = event.to_string();
                buffer = rest.to_string();
                for chunk in parse_sse_event(provider, &event_owned)? {
                    yield chunk;
                }
            }
        }

        // The connection closed. Some servers omit the trailing `\n\n`
        // on the last event, leaving a complete-but-unterminated payload
        // in the buffer. Parse it so callers do not silently lose the
        // final delta or `[DONE]` sentinel.
        let trailing = buffer.trim();
        if !trailing.is_empty() {
            for chunk in parse_sse_event(provider, trailing)? {
                yield chunk;
            }
        }
    };

    Box::pin(s)
}

/// Build the JSON body sent to `/v1/chat/completions`.
#[must_use]
pub fn build_request_payload(req: &GenerateRequest, stream: bool) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "model": req.model,
        "messages": req.messages.iter().map(message_to_openai).collect::<Vec<_>>(),
        "stream": stream,
        "response_format": { "type": "json_object" }
    });

    if !req.tools.is_empty() {
        payload["tools"] = req.tools.iter().map(tool_to_openai).collect();
    }
    if let Some(t) = req.temperature {
        payload["temperature"] = serde_json::json!(t);
    }
    if let Some(m) = req.max_tokens {
        payload["max_tokens"] = serde_json::json!(m);
    }
    if !req.stop_sequences.is_empty() {
        payload["stop"] = serde_json::json!(req.stop_sequences);
    }
    payload
}

fn message_to_openai(msg: &Message) -> serde_json::Value {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let text: String = msg
        .content
        .iter()
        .filter_map(|c| match c {
            super::types::Content::Text { text } => Some(text.as_str()),
            super::types::Content::ToolResult { content, .. } => Some(content.as_str()),
            super::types::Content::ToolUse { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("");
    serde_json::json!({ "role": role, "content": text })
}

fn tool_to_openai(tool: &ToolSchema) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters_schema,
        }
    })
}

/// Map a non-2xx HTTP response to the closest [`LlmError`] variant.
/// Reads the `Retry-After` response header on 429 responses.
#[must_use]
pub fn map_http_error(
    provider: &'static str,
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> LlmError {
    let preview: String = body.chars().take(256).collect();
    match status.as_u16() {
        401 | 403 => LlmError::AuthFailed {
            provider,
            message: preview,
        },
        429 => LlmError::RateLimited {
            provider,
            retry_after_seconds: headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok()),
        },
        400 => LlmError::InvalidResponse {
            provider,
            message: format!("bad request: {preview}"),
        },
        500..=599 => LlmError::ProviderUnavailable {
            provider,
            message: format!("HTTP {status}: {preview}"),
        },
        _ => LlmError::InvalidResponse {
            provider,
            message: format!("HTTP {status}: {preview}"),
        },
    }
}

/// Split off the first complete SSE event (terminated by `\n\n` or
/// `\r\n\r\n`) from `buffer`. Returns `(event, rest)` if found.
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

/// Parse one complete SSE event into zero or more `Chunk`s.
fn parse_sse_event(provider: &'static str, event: &str) -> Result<Vec<Chunk>, LlmError> {
    let mut out = Vec::new();
    for line in event.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            out.push(Chunk::Done {
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            });
            continue;
        }
        let parsed: OpenAiStreamChunk =
            serde_json::from_str(payload).map_err(|e| LlmError::InvalidResponse {
                provider,
                message: format!("invalid stream JSON: {e}"),
            })?;
        for chunk in openai_chunk_to_chunks(parsed) {
            out.push(chunk);
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    #[serde(default)]
    choices: Vec<OpenAiStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    #[serde(default)]
    delta: OpenAiDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAiToolCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    index: u32,
    #[serde(default)]
    function: Option<OpenAiToolFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

fn openai_chunk_to_chunks(chunk: OpenAiStreamChunk) -> Vec<Chunk> {
    let mut out = Vec::new();
    for choice in chunk.choices {
        if let Some(text) = choice.delta.content {
            if !text.is_empty() {
                out.push(Chunk::TextDelta(text));
            }
        }
        for call in choice.delta.tool_calls {
            let id = call
                .id
                .clone()
                .unwrap_or_else(|| format!("tool_{}", call.index));
            if let Some(function) = call.function {
                if let Some(name) = function.name {
                    out.push(Chunk::ToolCallStart {
                        id: id.clone(),
                        name,
                    });
                }
                if let Some(args) = function.arguments {
                    if !args.is_empty() {
                        out.push(Chunk::ToolCallArgsDelta {
                            id,
                            json_fragment: args,
                        });
                    }
                }
            }
        }
        if let Some(reason) = choice.finish_reason {
            let usage = chunk.usage.as_ref().map_or(Usage::default(), |u| Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            });
            out.push(Chunk::Done {
                usage,
                finish_reason: parse_finish_reason(&reason),
            });
        }
    }
    out
}

fn parse_finish_reason(raw: &str) -> FinishReason {
    match raw {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::MaxTokens,
        "tool_calls" | "function_call" => FinishReason::ToolUse,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::llm::types::Content;

    fn empty_request() -> GenerateRequest {
        GenerateRequest {
            model: "x".into(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            stop_sequences: Vec::new(),
        }
    }

    #[test]
    fn split_sse_event_handles_lf() {
        let buf = "data: a\n\ndata: b\n\nleftover";
        let (event, rest) = split_sse_event(buf).expect("first event");
        assert_eq!(event, "data: a");
        assert!(rest.starts_with("data: b"));
    }

    #[test]
    fn split_sse_event_handles_crlf() {
        let buf = "data: a\r\n\r\nrest";
        let (event, rest) = split_sse_event(buf).expect("event");
        assert_eq!(event, "data: a");
        assert_eq!(rest, "rest");
    }

    #[test]
    fn split_sse_event_returns_none_when_incomplete() {
        assert!(split_sse_event("data: incomplete").is_none());
    }

    #[test]
    fn parse_done_sentinel_emits_done_chunk() {
        let chunks = parse_sse_event("test", "data: [DONE]").expect("parse");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0], Chunk::Done { .. }));
    }

    #[test]
    fn parse_text_delta_chunk() {
        let payload = r#"data: {"choices":[{"delta":{"content":"hello"}}]}"#;
        let chunks = parse_sse_event("test", payload).expect("parse");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], Chunk::TextDelta("hello".into()));
    }

    #[test]
    fn parse_finish_reason_emits_done_with_usage() {
        let payload = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        let chunks = parse_sse_event("test", payload).expect("parse");
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            Chunk::Done {
                usage,
                finish_reason,
            } => {
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.output_tokens, 5);
                assert_eq!(*finish_reason, FinishReason::Stop);
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parse_invalid_json_returns_invalid_response_error() {
        let err = parse_sse_event("test", "data: {not json").expect_err("must fail");
        assert_eq!(err.code(), "LLM_INVALID_RESPONSE");
        assert_eq!(err.provider(), "test");
    }

    #[test]
    fn parse_finish_reason_maps_known_values() {
        assert_eq!(parse_finish_reason("stop"), FinishReason::Stop);
        assert_eq!(parse_finish_reason("length"), FinishReason::MaxTokens);
        assert_eq!(parse_finish_reason("tool_calls"), FinishReason::ToolUse);
        assert_eq!(
            parse_finish_reason("content_filter"),
            FinishReason::ContentFilter
        );
        assert_eq!(parse_finish_reason("unexpected"), FinishReason::Other);
    }

    #[test]
    fn build_request_payload_sets_required_fields() {
        let body = build_request_payload(&empty_request(), true);
        assert_eq!(body["model"], "x");
        assert_eq!(body["stream"], true);
        assert!(body["messages"].is_array());
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn build_request_payload_omits_empty_optionals() {
        let body = build_request_payload(&empty_request(), false);
        assert!(body.get("tools").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("stop").is_none());
    }

    #[test]
    fn message_to_openai_concats_text_blocks() {
        let msg = Message {
            role: Role::User,
            content: vec![Content::text("hello "), Content::text("world")],
        };
        let json = message_to_openai(&msg);
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hello world");
    }

    #[test]
    fn map_http_error_routes_status_codes() {
        let h = HeaderMap::new();
        assert_eq!(
            map_http_error("p", reqwest::StatusCode::UNAUTHORIZED, &h, "x").code(),
            "LLM_AUTH_FAILED"
        );
        assert_eq!(
            map_http_error("p", reqwest::StatusCode::TOO_MANY_REQUESTS, &h, "x").code(),
            "LLM_RATE_LIMITED"
        );
        assert_eq!(
            map_http_error("p", reqwest::StatusCode::BAD_REQUEST, &h, "x").code(),
            "LLM_INVALID_RESPONSE"
        );
        assert_eq!(
            map_http_error("p", reqwest::StatusCode::INTERNAL_SERVER_ERROR, &h, "x").code(),
            "LLM_PROVIDER_UNAVAILABLE"
        );
    }

    #[test]
    fn map_http_error_reads_retry_after_header() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", "42".parse().expect("header"));
        let err = map_http_error("p", reqwest::StatusCode::TOO_MANY_REQUESTS, &h, "");
        match err {
            LlmError::RateLimited {
                retry_after_seconds: Some(secs),
                ..
            } => assert_eq!(secs, 42),
            other => panic!("expected RateLimited with retry_after, got {other:?}"),
        }
    }
}
