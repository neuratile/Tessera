//! Generation service — ties RAG, prompts, and `LlmProvider` into a
//! single end-to-end artifact-production flow.
//!
//! Per `rules.md` §4.2 + §12.1 + §12.4 this is the *only* place
//! services / commands talk to the LLM. The flow:
//!
//! 1. Embed the scope hint via `EmbeddingProvider`.
//! 2. RAG retrieve top-K chunks via `chunk_repo::search_similar`.
//! 3. Pick the prompt module by [`ArtifactType`].
//! 4. Build messages + tool schema from `prompts::*::build_messages`.
//! 5. Token-budget check — refuse if the assembled prompt would
//!    exceed the model's `max_context_tokens`.
//! 6. `LlmProvider::stream` and aggregate the tool-call arguments.
//! 7. Validate the aggregated JSON against the prompt's tool schema
//!    via [`jsonschema`].
//! 8. Persist the artifact + generation metadata via `artifact_repo`.
//!
//! Streaming output is forwarded via a caller-supplied closure so the
//! Tauri command layer (Phase 6) can emit IPC events to the frontend
//! per chunk without coupling this service to Tauri.

use std::sync::Arc;

use chrono::Utc;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::prompts::{
    bug_report_v2, context_md_v1, defect_report_v2, test_cases_v2, test_plan_v2, PromptContext,
};
use crate::providers::embeddings::EmbeddingProvider;
use crate::providers::llm::types::{Chunk as LlmChunk, GenerateRequest, Message, ToolSchema};
use crate::providers::llm::{approximate_token_count, LlmProvider};
use crate::repositories::artifact_repo::{self, ArtifactInsert, ArtifactType, GenerationMetadata};
use crate::repositories::chunk_repo;
use crate::services::chunking_service::Chunk as CodeChunk;

/// Reserve at least this many tokens for the model's response so the
/// prompt cannot consume the entire context window. Doubles as the
/// generation `max_tokens` cap.
///
/// Sized to the largest structured payload we ask a model to emit:
/// `test_cases_v1` now returns the test cases *plus* a runnable `files[]`
/// array (source-under-test reproduced + a vitest spec per file), which
/// roughly doubled the test-cases output. At 4k the auth-fixture payload
/// truncated mid-array on the 3B CI model, leaving JSON the salvage path
/// could not balance and failing the golden probe — the same failure the
/// earlier 2k→4k bump fixed for the cases-only payload. 6k restores
/// headroom while staying well under the per-attempt stream timeout.
pub const RESPONSE_RESERVE_TOKENS: u32 = 6_000;

/// Top-K chunks pulled from the vector index for one generation.
/// Capped on the chunk-repo side too (`SEARCH_TOP_K_CAP = 50`).
pub const RAG_TOP_K: usize = 20;

/// Minimum non-zero similarity required for a hit to make it into
/// the prompt. Filters out entirely off-topic chunks the brute-force
/// cosine still returns when the query embedding is degenerate.
pub const MIN_SIMILARITY: f32 = 0.10;

/// Caller-facing request body. The Phase 6 IPC command layer
/// translates a JSON IPC payload into this struct.
#[derive(Debug, Clone)]
pub struct GenerationRequest {
    pub project_id: String,
    pub project_name: String,
    pub artifact_type: ArtifactType,
    pub model: String,
    /// Free-text scope description that drives RAG retrieval and is
    /// also surfaced in the prompt's `scope_hint`. Empty for
    /// whole-project artifacts.
    pub scope_hint: String,
    /// Project-level context.md content. Empty on the very first
    /// generation, in which case the consumer should run
    /// `ArtifactType::ContextMd` first.
    pub project_summary: String,
    /// Reviewer feedback from a prior regeneration cycle.
    pub reviewer_feedback: String,
    /// Parent artifact id when this call is a regeneration — bumps
    /// the version chain.
    pub parent_id: Option<String>,
}

/// Aggregated outcome returned to the caller. The `artifact_id` is
/// already persisted; consumers can fetch full details via
/// `artifact_repo::fetch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationOutcome {
    pub artifact_id: String,
    pub artifact_type: ArtifactType,
    pub structured_data: JsonValue,
    pub content_md: String,
    pub usage_input_tokens: u32,
    pub usage_output_tokens: u32,
}

/// Per-event hook the caller can supply to relay streaming progress
/// to the UI. The closure is called with each text or tool-args
/// fragment as it arrives. Forwarding is best-effort — generation
/// continues even if the closure errors.
pub type StreamSink = Box<dyn FnMut(StreamEvent) + Send>;

/// One progress event delivered to the [`StreamSink`]. Decoupled from
/// the LLM provider's `Chunk` enum so consumers do not need a
/// dependency on `crate::providers::llm`.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text delta (only meaningful for prompts that emit prose
    /// alongside the tool call — Phase 4 templates always force a
    /// tool call so this is rarely populated).
    Text(String),
    /// Tool-args JSON fragment as it streams in.
    ToolArgsDelta(String),
    /// Stream completed; carries final usage stats.
    Done {
        input_tokens: u32,
        output_tokens: u32,
    },
}

/// Collected references used by [`generate`]. Bundled into a struct
/// to keep the public function's parameter list short and to make
/// mocking in tests trivial.
pub struct GenerationDeps<'a> {
    pub pool: &'a SqlitePool,
    pub llm: Arc<dyn LlmProvider>,
    pub embeddings: Arc<dyn EmbeddingProvider>,
}

/// Run one end-to-end generation. Returns the persisted artifact's
/// id + structured payload.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] for empty `project_id` or `model`
///   strings.
/// - [`AppError::Llm`] propagated from the embedding or chat
///   provider.
/// - [`AppError::LimitExceeded`] when the assembled prompt cannot
///   fit in the model's context window with [`RESPONSE_RESERVE_TOKENS`]
///   left for output.
/// - [`AppError::Serde`] when the model's tool-call output cannot be
///   parsed as JSON.
/// - [`AppError::InvalidInput`] when the JSON validates to the wrong
///   shape per the prompt's `ToolSchema`.
/// - [`AppError::Database`] from any sqlx layer call.
pub async fn generate(
    request: GenerationRequest,
    deps: &GenerationDeps<'_>,
    mut on_event: Option<StreamSink>,
) -> AppResult<GenerationOutcome> {
    if request.project_id.trim().is_empty() {
        return Err(AppError::InvalidInput("project_id is empty".into()));
    }
    if request.model.trim().is_empty() {
        return Err(AppError::InvalidInput("model is empty".into()));
    }

    let started_at = Utc::now().to_rfc3339();

    // 1. Retrieve relevant chunks (skip when scope_hint is empty —
    //    the caller handed in a project_summary already and the
    //    template-level chunks list is allowed to be empty).
    let chunks = retrieve_chunks(&request, deps).await?;

    // 2. Build prompt + tool schema.
    let capabilities = deps.llm.capabilities();
    let budget = capabilities
        .max_context_tokens
        .saturating_sub(RESPONSE_RESERVE_TOKENS);

    let mut selected_chunks = Vec::new();
    for chunk in &chunks {
        let mut test_chunks = selected_chunks.clone();
        test_chunks.push(chunk.clone());
        let ctx = PromptContext {
            project_name: &request.project_name,
            project_summary: &request.project_summary,
            chunks: &test_chunks,
            scope_hint: &request.scope_hint,
            reviewer_feedback: &request.reviewer_feedback,
        };
        let (messages, _, _) = build_prompt(request.artifact_type, &ctx);
        let prompt_token_estimate = estimate_prompt_tokens(&messages);
        if prompt_token_estimate > budget {
            break;
        }
        selected_chunks.push(chunk.clone());
    }

    let ctx = PromptContext {
        project_name: &request.project_name,
        project_summary: &request.project_summary,
        chunks: &selected_chunks,
        scope_hint: &request.scope_hint,
        reviewer_feedback: &request.reviewer_feedback,
    };
    let (messages, tool_schema, prompt_version) = build_prompt(request.artifact_type, &ctx);

    // 3. Token budget — refuse before sending the request.
    let prompt_token_estimate = estimate_prompt_tokens(&messages);
    if prompt_token_estimate > budget {
        return Err(AppError::LimitExceeded(format!(
            "prompt {prompt_token_estimate} tokens exceeds context budget {budget} (model {})",
            request.model,
        )));
    }

    // 4. Stream + aggregate.
    let llm_request = GenerateRequest {
        model: request.model.clone(),
        messages,
        tools: vec![tool_schema.clone()],
        temperature: Some(0.2),
        max_tokens: Some(RESPONSE_RESERVE_TOKENS),
        stop_sequences: Vec::new(),
    };
    let aggregated = drive_stream(deps.llm.as_ref(), llm_request, on_event.as_mut()).await?;

    // 5. Parse the structured payload.
    let raw_json = extract_raw_json(&aggregated, &tool_schema, &request.model)?;

    let mut structured_data: JsonValue =
        serde_json::from_str(&raw_json).map_err(AppError::Serde)?;
    normalize_missing_arrays(&mut structured_data, &tool_schema);
    validate_tool_output(&tool_schema, &structured_data)?;
    let input_tokens = aggregated.input_tokens;
    let output_tokens = aggregated.output_tokens;

    // 6. Persist.
    let completed_at = Utc::now().to_rfc3339();
    let title = derive_title(&request, &structured_data);
    let content_md = render_markdown(request.artifact_type, &structured_data);

    let metadata = GenerationMetadata {
        provider: deps.llm.name().to_string(),
        model: request.model.clone(),
        prompt_version: prompt_version.to_string(),
        input_tokens,
        output_tokens,
        started_at,
        completed_at,
    };

    let id = artifact_repo::insert(
        deps.pool,
        ArtifactInsert {
            project_id: request.project_id.clone(),
            artifact_type: request.artifact_type,
            title,
            content_md: content_md.clone(),
            structured_data: structured_data.clone(),
            generation_metadata: metadata,
            parent_id: request.parent_id.clone(),
        },
    )
    .await?;

    Ok(GenerationOutcome {
        artifact_id: id,
        artifact_type: request.artifact_type,
        structured_data,
        content_md,
        usage_input_tokens: input_tokens,
        usage_output_tokens: output_tokens,
    })
}

/// Resolve the raw JSON string from a completed stream aggregate.
/// Preferred path: tool-call args. Fallback: salvage a JSON object from
/// free text (common with small Ollama models). Both empty: descriptive
/// error suggesting a tool-capable model.
fn extract_raw_json(
    aggregated: &StreamAggregate,
    tool_schema: &ToolSchema,
    model: &str,
) -> AppResult<String> {
    if !aggregated.tool_args.trim().is_empty() {
        return Ok(aggregated.tool_args.clone());
    }
    if let Some(salvaged) = salvage_tool_args(&aggregated.text, &tool_schema.name) {
        tracing::warn!(
            model = %model,
            text_len = aggregated.text_len,
            "model emitted JSON as free text instead of invoking the tool — salvaging"
        );
        return Ok(salvaged);
    }
    if aggregated.text_len == 0 {
        return Err(AppError::InvalidInput(format!(
            "model `{model}` returned an empty response. Tool calling may not be \
             supported by this model — try `qwen2.5-coder:7b`, `qwen2.5:14b`, \
             or a cloud model like `gpt-4o-mini` / `claude-3-5-sonnet`."
        )));
    }
    let preview: String = aggregated.text.chars().take(200).collect();
    let tool_name = &tool_schema.name;
    let text_len = aggregated.text_len;
    // Some local models ignore the OpenAI-compatible `tools` field and
    // print their native pseudo-call syntax as plain text. The salvage
    // path above handles recoverable object literals; this branch keeps
    // unrecoverable pseudo-calls actionable instead of generic.
    if let Some(fmt) = detect_non_tool_call_format(&aggregated.text) {
        return Err(AppError::InvalidInput(format!(
            "model `{model}` did not invoke `{tool_name}`; it emitted {fmt} as plain text. \
             Use a tool-capable model or a model that can emit strict JSON. Preview: {preview}"
        )));
    }
    Err(AppError::InvalidInput(format!(
        "model `{model}` did not invoke `{tool_name}` and emitted {text_len} chars of free text \
         that does not contain a JSON object. Preview: {preview}"
    )))
}

fn detect_non_tool_call_format(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.contains("tool_code") {
        Some("Gemma-style `tool_code` blocks")
    } else if lower.contains("<tool_call") {
        Some("`<tool_call>` tags")
    } else if lower.contains("<function_call") || lower.contains("<|python_tag|>") {
        Some("Llama function-call tags")
    } else if lower.contains("default_api.") {
        Some("a code snippet")
    } else {
        None
    }
}

/// Result of draining the LLM stream — extracted so [`generate`] stays
/// inside the clippy `too_many_lines` budget.
struct StreamAggregate {
    tool_args: String,
    /// Accumulated free-text response. Most prompts force a tool
    /// call, so this is normally empty — but small / non-tool-trained
    /// models (e.g. `gemma:2b`, `llama3.2:1b`) ignore the `tools`
    /// field and emit a JSON object as plain text. The salvage path
    /// in [`generate`] tries to parse this as JSON before failing.
    text: String,
    text_len: usize,
    input_tokens: u32,
    output_tokens: u32,
}

async fn drive_stream(
    llm: &dyn LlmProvider,
    request: GenerateRequest,
    mut sink: Option<&mut StreamSink>,
) -> AppResult<StreamAggregate> {
    let mut tool_args = String::new();
    let mut text = String::new();
    let mut text_len: usize = 0;
    let mut input_tokens = 0_u32;
    let mut output_tokens = 0_u32;

    let mut stream = llm.stream(request);
    while let Some(item) = stream.next().await {
        match item? {
            LlmChunk::TextDelta(delta) => {
                text_len = text_len.saturating_add(delta.len());
                text.push_str(&delta);
                if let Some(s) = sink.as_deref_mut() {
                    s(StreamEvent::Text(delta));
                }
            }
            LlmChunk::ToolCallStart { .. } => {}
            LlmChunk::ToolCallArgsDelta { json_fragment, .. } => {
                tool_args.push_str(&json_fragment);
                if let Some(s) = sink.as_deref_mut() {
                    s(StreamEvent::ToolArgsDelta(json_fragment));
                }
            }
            LlmChunk::Done {
                usage,
                finish_reason: _,
            } => {
                input_tokens = usage.input_tokens;
                output_tokens = usage.output_tokens;
                if let Some(s) = sink.as_deref_mut() {
                    s(StreamEvent::Done {
                        input_tokens,
                        output_tokens,
                    });
                }
            }
        }
    }

    Ok(StreamAggregate {
        tool_args,
        text,
        text_len,
        input_tokens,
        output_tokens,
    })
}

/// Try to extract a JSON object from a raw text response. Small
/// models often ignore the `tools` field and emit the JSON as plain
/// text — sometimes wrapped in a `json` fenced code block, sometimes
/// prefixed with prose, sometimes with a trailing comment. This
/// salvage path finds the outermost balanced `{...}` and returns it
/// for downstream schema validation.
/// Try to salvage tool-call arguments out of a free-text response.
///
/// Three shapes are observed from small / non-tool-trained models:
///
///  1. Direct payload: `{"summary": "...", "cases": [...]}`
///  2. Tool-call wrapper that names the expected tool:
///     `{"name": "emit_test_plan", "arguments": {...}}`. Some Ollama
///     models emit this when they "understand" `tools` but cannot
///     route it through the protocol — we unwrap to the inner
///     `arguments` object so downstream JSON-Schema validation sees
///     the same shape as a successful tool call.
///  3. Same wrapper but the model wraps a *case* (not the whole
///     payload): `{"name": "TC-1", "arguments": {...}}`. In that
///     case we cannot recover — the salvaged JSON is a fragment of
///     the expected `cases` array. Return `None` so the caller
///     surfaces a clear "swap model" error.
///
/// Returns the JSON string ready for `validate_tool_output`.
#[allow(clippy::too_many_lines)]
pub(crate) fn salvage_tool_args(text: &str, tool_name: &str) -> Option<String> {
    let raw = salvage_json_from_text(text).or_else(|| salvage_js_object_literal_from_text(text))?;
    let mut parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;

    if let Some(obj) = parsed.as_object_mut() {
        let name_key = if obj.contains_key("name") {
            Some("name")
        } else if obj.contains_key("function") {
            Some("function")
        } else if obj.contains_key("function_name") {
            Some("function_name")
        } else if obj.contains_key("tool") {
            Some("tool")
        } else if obj.contains_key("tool_name") {
            Some("tool_name")
        } else {
            None
        };

        if let Some(n_key) = name_key {
            if let Some(name_str) = obj.get(n_key).and_then(|v| v.as_str()) {
                if name_str == tool_name || name_str.contains(tool_name) {
                    // Check if there is an explicit payload key
                    let mut payload_key = None;
                    let candidate_keys = [
                        "arguments",
                        "args",
                        "parameters",
                        "params",
                        "report",
                        "payload",
                        "data",
                        "body",
                        "value",
                        "result",
                    ];
                    for ck in candidate_keys {
                        if obj.contains_key(ck) {
                            payload_key = Some(ck);
                            break;
                        }
                    }

                    // If no explicit payload key, check if there is exactly 1 other key in the object,
                    // and its value is a JSON object or array.
                    if payload_key.is_none() {
                        let other_keys: Vec<&String> =
                            obj.keys().filter(|k| k.as_str() != n_key).collect();
                        if other_keys.len() == 1 {
                            let key = other_keys[0];
                            if obj.get(key).is_some_and(|v| v.is_object() || v.is_array()) {
                                payload_key = Some(key.as_str());
                            }
                        }
                    }

                    if let Some(p_key) = payload_key {
                        // Extract the unwrapped payload
                        if let Some(payload_val) = obj.get(p_key) {
                            parsed = payload_val.clone();
                        }
                    } else {
                        // Inline payload: the payload fields are at the top level alongside the tool name.
                        // Just remove the tool name key (and other wrapper metadata keys)
                        obj.remove(n_key);
                        obj.remove("id");
                        obj.remove("type");
                    }
                } else {
                    // Wrapper targets something other than the expected tool (e.g. TC-1)
                    return None;
                }
            }
        }
    }

    // 1. Array wrapping: if the parsed JSON is an array, wrap it in the expected object schema
    if parsed.is_array() {
        let mut new_obj = serde_json::Map::new();
        if tool_name == "emit_bug_report" {
            new_obj.insert("bugs".to_string(), parsed.clone());
            parsed = serde_json::Value::Object(new_obj);
        } else if tool_name == "emit_defect_report" {
            new_obj.insert("findings".to_string(), parsed.clone());
            parsed = serde_json::Value::Object(new_obj);
        } else if tool_name == "emit_test_cases" {
            new_obj.insert("cases".to_string(), parsed.clone());
            parsed = serde_json::Value::Object(new_obj);
        }
    }

    // 2. Object wrapping & field remapping
    if let Some(obj) = parsed.as_object_mut() {
        // If the model wrapped a single item directly in the root object
        if tool_name == "emit_bug_report"
            && !obj.contains_key("bugs")
            && (obj.contains_key("title") || obj.contains_key("id") || obj.contains_key("severity"))
        {
            let mut new_obj = serde_json::Map::new();
            new_obj.insert(
                "bugs".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::Object(obj.clone())]),
            );
            *obj = new_obj;
        }
        if tool_name == "emit_defect_report"
            && !obj.contains_key("findings")
            && (obj.contains_key("title")
                || obj.contains_key("id")
                || obj.contains_key("severity")
                || obj.contains_key("category"))
        {
            let mut new_obj = serde_json::Map::new();
            new_obj.insert(
                "findings".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::Object(obj.clone())]),
            );
            *obj = new_obj;
        }
        if tool_name == "emit_test_cases"
            && !obj.contains_key("cases")
            && (obj.contains_key("title") || obj.contains_key("id") || obj.contains_key("steps"))
        {
            let mut new_obj = serde_json::Map::new();
            new_obj.insert(
                "cases".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::Object(obj.clone())]),
            );
            *obj = new_obj;
        }

        // Strip extra wrapper keys around the expected payload structure
        if tool_name == "emit_bug_report" && obj.contains_key("bugs") {
            let mut new_obj = serde_json::Map::new();
            if let Some(val) = obj.get("bugs") {
                new_obj.insert("bugs".to_string(), val.clone());
            }
            *obj = new_obj;
        }
        if tool_name == "emit_defect_report" && obj.contains_key("findings") {
            let mut new_obj = serde_json::Map::new();
            if let Some(val) = obj.get("findings") {
                new_obj.insert("findings".to_string(), val.clone());
            }
            if let Some(val) = obj.get("summary") {
                new_obj.insert("summary".to_string(), val.clone());
            }
            *obj = new_obj;
        }
        if tool_name == "emit_test_cases" && obj.contains_key("cases") {
            let mut new_obj = serde_json::Map::new();
            if let Some(val) = obj.get("cases") {
                let mut cases_array = val.clone();
                if let Some(cases) = cases_array.as_array_mut() {
                    for case in cases {
                        if let Some(case_obj) = case.as_object_mut() {
                            if let Some(trace_val) = case_obj.get_mut("traceability") {
                                if trace_val.is_string() {
                                    *trace_val = serde_json::Value::Array(vec![trace_val.clone()]);
                                } else if let Some(trace_obj) = trace_val.as_object() {
                                    let file_hint = trace_obj
                                        .get("file_hint")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let symbol = trace_obj
                                        .get("symbol")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let trace_str = if !file_hint.is_empty() && !symbol.is_empty() {
                                        format!("{file_hint}#{symbol}")
                                    } else if !file_hint.is_empty() {
                                        file_hint.to_string()
                                    } else {
                                        symbol.to_string()
                                    };
                                    *trace_val =
                                        serde_json::Value::Array(vec![serde_json::Value::String(
                                            trace_str,
                                        )]);
                                }
                            }
                        }
                    }
                }
                new_obj.insert("cases".to_string(), cases_array);
            }
            *obj = new_obj;
        }

        // Re-mapping for emit_project_context
        if tool_name == "emit_project_context" {
            let has_summary = obj.contains_key("summary");
            let has_notes = obj.contains_key("architecture_notes");
            if !has_summary || !has_notes {
                let mut title = String::new();
                let mut description = String::new();
                let mut category = String::new();
                let mut other_notes = Vec::new();

                for (k, v) in obj.iter() {
                    let k_lower = k.to_lowercase();
                    if k_lower.contains("title") || k_lower.contains("name") {
                        if let Some(s) = v.as_str() {
                            title = s.to_string();
                        } else {
                            title = v.to_string();
                        }
                    } else if k_lower.contains("desc") || k_lower.contains("summary") {
                        if let Some(s) = v.as_str() {
                            description = s.to_string();
                        } else {
                            description = v.to_string();
                        }
                    } else if k_lower.contains("category") || k_lower.contains("type") {
                        if let Some(s) = v.as_str() {
                            category = s.to_string();
                        } else {
                            category = v.to_string();
                        }
                    } else if k_lower != "key_modules"
                        && k_lower != "data_flows"
                        && k_lower != "known_risks"
                    {
                        let v_str = if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            v.to_string()
                        };
                        other_notes.push(format!("* {k}: {v_str}"));
                    }
                }

                let salvaged_summary = if has_summary {
                    obj.get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    let mut s = String::new();
                    if !title.is_empty() {
                        s.push_str("Project: ");
                        s.push_str(&title);
                        s.push_str(". ");
                    }
                    if !category.is_empty() {
                        s.push_str("Category: ");
                        s.push_str(&category);
                        s.push_str(". ");
                    }
                    if description.is_empty() {
                        s.push_str(
                            "A structured summary of the codebase components and architecture.",
                        );
                    } else {
                        s.push_str(&description);
                    }
                    s
                };

                let salvaged_notes = if has_notes {
                    obj.get("architecture_notes")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                } else if other_notes.is_empty() {
                    "* Codebase details and architectural overview compiled from sampled components.".to_string()
                } else {
                    other_notes.join("\n")
                };

                let mut new_obj = serde_json::Map::new();
                if let Some(val) = obj.get("key_modules") {
                    new_obj.insert("key_modules".to_string(), val.clone());
                }
                if let Some(val) = obj.get("data_flows") {
                    new_obj.insert("data_flows".to_string(), val.clone());
                }
                if let Some(val) = obj.get("known_risks") {
                    new_obj.insert("known_risks".to_string(), val.clone());
                }
                new_obj.insert(
                    "summary".to_string(),
                    serde_json::Value::String(salvaged_summary),
                );
                new_obj.insert(
                    "architecture_notes".to_string(),
                    serde_json::Value::String(salvaged_notes),
                );
                *obj = new_obj;
            }
        }

        // Re-mapping for emit_test_plan
        if tool_name == "emit_test_plan" {
            // The v2 schema nests scope as `{ inScope, outOfScope }` and
            // rejects unknown keys, so always remap the flat v1-style
            // `scopeIn`/`scopeOut` keys models still emit — the generic
            // re-nest normalization cannot match them by key name.
            if !obj.contains_key("scope")
                && (obj.contains_key("scopeIn") || obj.contains_key("scopeOut"))
            {
                let empty_array = || serde_json::Value::Array(Vec::new());
                let mut scope_obj = serde_json::Map::new();
                scope_obj.insert(
                    "inScope".to_string(),
                    obj.remove("scopeIn").unwrap_or_else(empty_array),
                );
                scope_obj.insert(
                    "outOfScope".to_string(),
                    obj.remove("scopeOut").unwrap_or_else(empty_array),
                );
                obj.insert("scope".to_string(), serde_json::Value::Object(scope_obj));
            }

            let has_summary = obj.contains_key("summary");
            let has_strategy = obj.contains_key("strategy");
            if !has_summary || !has_strategy {
                let mut title = String::new();
                let mut description = String::new();
                let mut other_notes = Vec::new();

                for (k, v) in obj.iter() {
                    let k_lower = k.to_lowercase();
                    if k_lower.contains("title") || k_lower.contains("name") {
                        if let Some(s) = v.as_str() {
                            title = s.to_string();
                        } else {
                            title = v.to_string();
                        }
                    } else if k_lower.contains("desc")
                        || k_lower.contains("summary")
                        || k_lower.contains("strategy")
                    {
                        if let Some(s) = v.as_str() {
                            description = s.to_string();
                        } else {
                            description = v.to_string();
                        }
                    } else {
                        let v_str = if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            v.to_string()
                        };
                        other_notes.push(format!("* {k}: {v_str}"));
                    }
                }

                let salvaged_summary = if has_summary {
                    obj.get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                } else if !description.is_empty() {
                    description.clone()
                } else if !title.is_empty() {
                    format!("Test plan for project: {title}")
                } else {
                    "Structured test plan detailing high-level test strategy and criteria."
                        .to_string()
                };

                let salvaged_strategy = if has_strategy {
                    obj.get("strategy")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                } else if other_notes.is_empty() {
                    "Perform functional integration and unit verification based on the components."
                        .to_string()
                } else {
                    format!(
                        "Verify the following architectural aspects:\n{}",
                        other_notes.join("\n")
                    )
                };

                let mut new_obj = serde_json::Map::new();
                new_obj.insert(
                    "summary".to_string(),
                    serde_json::Value::String(salvaged_summary),
                );
                new_obj.insert(
                    "strategy".to_string(),
                    serde_json::Value::String(salvaged_strategy),
                );

                let array_keys = [
                    "objectives",
                    "testLevels",
                    "testTypes",
                    "environments",
                    "risks",
                    "entryCriteria",
                    "exitCriteria",
                    "suspensionCriteria",
                    "deliverables",
                ];
                for ak in array_keys {
                    if let Some(val) = obj.get(ak) {
                        new_obj.insert(ak.to_string(), val.clone());
                    }
                }

                // `scope` is a required object, and the missing-array
                // normalization only backfills arrays — carry it over (it
                // was re-nested above when only flat keys were present) or
                // insert an empty shell so v2 validation passes.
                if let Some(val) = obj.get("scope") {
                    new_obj.insert("scope".to_string(), val.clone());
                } else {
                    let mut scope_obj = serde_json::Map::new();
                    scope_obj.insert(
                        "inScope".to_string(),
                        serde_json::Value::Array(Vec::new()),
                    );
                    scope_obj.insert(
                        "outOfScope".to_string(),
                        serde_json::Value::Array(Vec::new()),
                    );
                    new_obj.insert("scope".to_string(), serde_json::Value::Object(scope_obj));
                }
                *obj = new_obj;
            }
        }
    }

    serde_json::to_string(&parsed).ok()
}

pub(crate) fn strip_think_blocks(text: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;

    while let Some(start_idx) = find_case_insensitive(remaining, "<think>") {
        result.push_str(&remaining[..start_idx]);
        let post_start = &remaining[start_idx..];

        if let Some(end_idx) = find_case_insensitive(post_start, "</think>") {
            let skip_len = end_idx + 8;
            remaining = &post_start[skip_len..];
        } else {
            remaining = "";
            break;
        }
    }
    result.push_str(remaining);
    result
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle_len = needle.len();
    if needle_len == 0 {
        return Some(0);
    }
    for i in 0..=haystack.len().saturating_sub(needle_len) {
        if haystack.is_char_boundary(i)
            && haystack.is_char_boundary(i + needle_len)
            && haystack[i..i + needle_len].eq_ignore_ascii_case(needle)
        {
            return Some(i);
        }
    }
    None
}

#[allow(clippy::needless_range_loop)]
fn find_balanced_braces(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut start = 0;

    while start < len {
        if bytes[start] == b'{' {
            let mut depth = 0_i32;
            let mut in_string: Option<u8> = None;
            let mut escaped = false;
            let mut matched_end = None;
            for i in start..len {
                let b = bytes[i];
                if let Some(quote) = in_string {
                    if escaped {
                        escaped = false;
                    } else if b == b'\\' {
                        escaped = true;
                    } else if b == quote {
                        in_string = None;
                    }
                    continue;
                }
                match b {
                    b'\'' | b'"' => in_string = Some(b),
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            matched_end = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(end_idx) = matched_end {
                if let Some(slice) = text.get(start..=end_idx) {
                    blocks.push(slice.to_string());
                }
                start = end_idx + 1;
                continue;
            }
        }
        start += 1;
    }
    blocks
}

pub(crate) fn salvage_json_from_text(text: &str) -> Option<String> {
    let cleaned = strip_think_blocks(text);
    let blocks = find_balanced_braces(&cleaned);

    blocks
        .into_iter()
        .rev()
        .find(|block| serde_json::from_str::<serde_json::Value>(block).is_ok())
}

fn salvage_js_object_literal_from_text(text: &str) -> Option<String> {
    let cleaned = strip_think_blocks(text);
    let blocks = find_balanced_braces(&cleaned);

    blocks
        .into_iter()
        .rev()
        .filter_map(|block| js_object_literal_to_json(&block))
        .find(|json| serde_json::from_str::<serde_json::Value>(json).is_ok())
}

fn js_object_literal_to_json(raw: &str) -> Option<String> {
    let normalized_strings = normalize_js_strings(raw);
    let quoted_keys = quote_js_object_keys(&normalized_strings);
    let without_trailing_commas = remove_js_trailing_commas(&quoted_keys);
    if serde_json::from_str::<serde_json::Value>(&without_trailing_commas).is_ok() {
        Some(without_trailing_commas)
    } else {
        None
    }
}

fn normalize_js_strings(raw: &str) -> String {
    let mut out = String::new();
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for ch in raw.chars() {
        if let Some(quote) = in_string {
            if escaped {
                // `\'` is valid in JS strings (single- or double-quoted)
                // but not in JSON; pop the `\` we already wrote and emit
                // the quote bare.
                if ch == '\'' {
                    out.pop();
                }
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                out.push(ch);
                escaped = true;
            } else if ch == quote {
                out.push('"');
                in_string = None;
            } else if quote == '\'' && ch == '"' {
                out.push_str("\\\"");
            } else {
                out.push(ch);
            }
            continue;
        }

        if ch == '\'' || ch == '"' {
            out.push('"');
            in_string = Some(ch);
        } else {
            out.push(ch);
        }
    }

    out
}

fn quote_js_object_keys(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;

    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }

        if ch != '{' && ch != ',' {
            out.push(ch);
            i += 1;
            continue;
        }

        out.push(ch);
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            out.push(chars[i]);
            i += 1;
        }

        if i >= chars.len() || !is_js_identifier_start(chars[i]) {
            continue;
        }

        let key_start = i;
        i += 1;
        while i < chars.len() && is_js_identifier_continue(chars[i]) {
            i += 1;
        }
        let key: String = chars[key_start..i].iter().collect();
        let mut j = i;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }

        if j < chars.len() && chars[j] == ':' {
            out.push('"');
            out.push_str(&key);
            out.push('"');
        } else {
            out.push_str(&key);
        }
    }

    out
}

fn is_js_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}

fn is_js_identifier_continue(ch: char) -> bool {
    is_js_identifier_start(ch) || ch.is_ascii_digit()
}

fn remove_js_trailing_commas(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;

    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    out
}

#[allow(clippy::too_many_lines)]
async fn retrieve_chunks(
    request: &GenerationRequest,
    deps: &GenerationDeps<'_>,
) -> AppResult<Vec<CodeChunk>> {
    // Every artifact type needs concrete symbols in the prompt or the
    // model emits an apologetic "no code provided" payload (Context /
    // TestPlan) or an empty `cases` / `defects` / `bugs` array that
    // fails JSON-Schema validation (TestCases / DefectReport /
    // BugReport). When the caller does not supply a `scope_hint`,
    // fall back to a generic phrase so RAG returns *some* chunks
    // instead of an empty list. ContextMd + TestPlan get a slightly
    // broader phrase ("project overview, core modules…") because the
    // prompts are whole-project rather than scope-targeted.
    let scope_provided = !request.scope_hint.trim().is_empty();
    let effective_query = if scope_provided {
        request.scope_hint.clone()
    } else {
        match request.artifact_type {
            ArtifactType::ContextMd | ArtifactType::TestPlan => format!(
                "project overview, core modules, public APIs, exported functions, \
                 classes, and entry points of {}",
                request.project_name
            ),
            ArtifactType::TestCases | ArtifactType::DefectReport | ArtifactType::BugReport => {
                format!(
                    "core public APIs, exported functions, classes, and entry points of {}",
                    request.project_name
                )
            }
        }
    };

    if effective_query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut embeddings = match deps.embeddings.embed(vec![effective_query.clone()]).await {
        Ok(emb) => emb,
        Err(crate::providers::llm::error::LlmError::ConnectionFailed { provider: "ollama", message }) => {
            tracing::warn!("Local Ollama embedding server is offline. Attempting auto-start...");

            let mut cmd = std::process::Command::new("ollama");
            cmd.arg("serve");
            crate::utils::process::configure_detached_process(&mut cmd);

            if cmd.spawn().is_ok() {
                let mut retry_success = false;
                let mut last_err = message;
                let mut resolved_embeddings = Vec::new();

                for i in 0..10 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    match deps.embeddings.embed(vec![effective_query.clone()]).await {
                        Ok(emb) => {
                            tracing::info!("Ollama server auto-started successfully after {}s.", i + 1);
                            resolved_embeddings = emb;
                            retry_success = true;
                            break;
                        }
                        Err(crate::providers::llm::error::LlmError::ConnectionFailed { message: err_msg, .. }) => {
                            last_err = err_msg;
                        }
                        Err(other_err) => {
                            return Err(AppError::Llm(other_err));
                        }
                    }
                }

                if !retry_success {
                    return Err(AppError::Llm(crate::providers::llm::error::LlmError::ConnectionFailed {
                        provider: "ollama",
                        message: format!(
                            "Local Ollama server is not running and could not be started automatically. \
                             Please verify Ollama is installed, or open Settings and click 'Start Server' for Ollama (local) to enable codebase search. \
                             Original error: {last_err}"
                        ),
                    }));
                }
                resolved_embeddings
            } else {
                return Err(AppError::Llm(crate::providers::llm::error::LlmError::ConnectionFailed {
                    provider: "ollama",
                    message: format!(
                        "Local Ollama server is not running and failed to auto-start (verify 'ollama' is in your PATH). \
                         Please open Settings and click 'Start Server' for Ollama (local). \
                         Original error: {message}"
                    ),
                }));
            }
        }
        Err(other_err) => return Err(AppError::Llm(other_err)),
    };

    let Some(query_vec) = embeddings.pop() else {
        return Ok(Vec::new());
    };

    let dim = u32::try_from(deps.embeddings.dimension()).unwrap_or(u32::MAX);
    let provider_name = format!("{}-{}", deps.embeddings.name(), deps.embeddings.model_id());
    let hits = chunk_repo::search_similar(
        deps.pool,
        &request.project_id,
        &provider_name,
        dim,
        &query_vec,
        RAG_TOP_K,
    )
    .await?;

    // When the caller supplied a real scope_hint we trust it and apply
    // the similarity floor to drop off-topic chunks. With the fallback
    // generic query, drop the floor so we always surface the top-K
    // chunks rather than risk an empty prompt for code-grounded
    // artifacts.
    let filtered: Vec<_> = if scope_provided {
        hits.into_iter()
            .filter(|h| h.similarity >= MIN_SIMILARITY)
            .collect()
    } else {
        hits
    };

    Ok(filtered
        .into_iter()
        .map(|h| CodeChunk {
            kind: h.kind,
            name: h.name,
            start_line: h.start_line,
            end_line: h.end_line,
            content: h.content,
            token_count: h.token_count as usize,
            oversize: false,
        })
        .collect())
}

fn build_prompt(
    kind: ArtifactType,
    ctx: &PromptContext<'_>,
) -> (Vec<Message>, ToolSchema, &'static str) {
    match kind {
        ArtifactType::ContextMd => (
            context_md_v1::build_messages(ctx),
            context_md_v1::tool(),
            context_md_v1::VERSION,
        ),
        ArtifactType::TestPlan => (
            test_plan_v2::build_messages(ctx),
            test_plan_v2::tool(),
            test_plan_v2::VERSION,
        ),
        ArtifactType::TestCases => (
            test_cases_v2::build_messages(ctx),
            test_cases_v2::tool(),
            test_cases_v2::VERSION,
        ),
        ArtifactType::DefectReport => (
            defect_report_v2::build_messages(ctx),
            defect_report_v2::tool(),
            defect_report_v2::VERSION,
        ),
        ArtifactType::BugReport => (
            bug_report_v2::build_messages(ctx),
            bug_report_v2::tool(),
            bug_report_v2::VERSION,
        ),
    }
}

fn estimate_prompt_tokens(messages: &[Message]) -> u32 {
    let mut total: usize = 0;
    for m in messages {
        for c in &m.content {
            if let crate::providers::llm::types::Content::Text { text } = c {
                total = total.saturating_add(approximate_token_count(text));
            }
        }
    }
    u32::try_from(total).unwrap_or(u32::MAX)
}

fn normalize_key_name(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Second re-nest pass for keys that belong to an array-of-objects item
/// schema: small models flatten a single `steps[]` entry's `action` /
/// `expectedResult` onto the test case itself, which
/// `additionalProperties: false` rejects. Lift every unknown key claimed
/// by exactly one array-of-objects property into a single new element of
/// that array — but only when the array is absent or empty, so elements
/// the model actually emitted are never mutated. Ambiguous keys (claimed
/// by more than one array property) are left for validation to report.
fn renest_flattened_array_items(
    obj: &mut serde_json::Map<String, JsonValue>,
    properties: &serde_json::Map<String, JsonValue>,
) {
    let unknown_keys: Vec<String> = obj
        .keys()
        .filter(|k| !properties.contains_key(*k))
        .cloned()
        .collect();

    let mut lifted: std::collections::BTreeMap<String, serde_json::Map<String, JsonValue>> =
        std::collections::BTreeMap::new();
    for key in unknown_keys {
        let norm_key = normalize_key_name(&key);
        let mut owners = properties.iter().filter(|(_, prop_schema)| {
            prop_schema.get("type").and_then(|t| t.as_str()) == Some("array")
                && prop_schema
                    .get("items")
                    .and_then(|i| i.get("properties"))
                    .and_then(|p| p.as_object())
                    .is_some_and(|nested| {
                        nested.keys().any(|nk| normalize_key_name(nk) == norm_key)
                    })
        });
        let Some((owner_name, _)) = owners.next() else {
            continue;
        };
        if owners.next().is_some() {
            continue;
        }
        let owner_name = owner_name.clone();
        let target_is_liftable = match obj.get(&owner_name) {
            None => true,
            Some(JsonValue::Array(existing)) => existing.is_empty(),
            Some(_) => false,
        };
        if !target_is_liftable {
            continue;
        }
        if let Some(val) = obj.remove(&key) {
            lifted.entry(owner_name).or_default().insert(key, val);
        }
    }

    for (owner_name, item) in lifted {
        obj.insert(owner_name, JsonValue::Array(vec![JsonValue::Object(item)]));
    }
}

/// Coerce structured values in string-typed fields: small models emit
/// JSON objects for fields like `testData` (`{"email": "...",
/// "password": "..."}`) where the schema wants a string. Serialize the
/// value to its compact JSON text so a richer-but-wrong-shaped value
/// validates instead of hard-failing the generation. Nulls are left for
/// validation to report.
fn coerce_structured_strings(
    obj: &mut serde_json::Map<String, JsonValue>,
    properties: &serde_json::Map<String, JsonValue>,
) {
    for (schema_key, property_schema) in properties {
        if property_schema.get("type").and_then(|t| t.as_str()) != Some("string") {
            continue;
        }
        if let Some(val) = obj.get_mut(schema_key) {
            if !matches!(val, JsonValue::String(_) | JsonValue::Null) {
                let text = val.to_string();
                *val = JsonValue::String(text);
            }
        }
    }
}

fn normalize_value_recursively(data: &mut JsonValue, schema: &JsonValue) {
    match data {
        JsonValue::Object(obj) => {
            let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) else {
                return;
            };

            // 1. Casing normalization: Rename variant keys to exact schema keys
            let mut keys_to_rename = Vec::new();
            for schema_key in properties.keys() {
                if obj.contains_key(schema_key) {
                    continue;
                }
                let norm_schema = normalize_key_name(schema_key);
                for obj_key in obj.keys() {
                    if normalize_key_name(obj_key) == norm_schema {
                        keys_to_rename.push((obj_key.clone(), schema_key.clone()));
                        break;
                    }
                }
            }

            for (old_key, new_key) in keys_to_rename {
                if let Some(val) = obj.remove(&old_key) {
                    obj.insert(new_key, val);
                }
            }

            // 2. Re-nest flattened object fields: models sometimes emit the
            // keys of a nested object (e.g. `location.symbol` on a defect
            // finding, `rootCause.symbol` on a bug) at the parent level,
            // which `additionalProperties: false` rejects. Move any key
            // that is unknown at this level but declared by exactly one
            // object-typed property into that object. Ambiguous keys
            // (claimed by two nested objects) are left for validation to
            // report rather than guessed at.
            let unknown_keys: Vec<String> = obj
                .keys()
                .filter(|k| !properties.contains_key(*k))
                .cloned()
                .collect();
            for key in unknown_keys {
                let norm_key = normalize_key_name(&key);
                let mut owners = properties.iter().filter(|(_, prop_schema)| {
                    prop_schema.get("type").and_then(|t| t.as_str()) == Some("object")
                        && prop_schema
                            .get("properties")
                            .and_then(|p| p.as_object())
                            .is_some_and(|nested| {
                                nested.keys().any(|nk| normalize_key_name(nk) == norm_key)
                            })
                });
                let Some((owner_name, _)) = owners.next() else {
                    continue;
                };
                if owners.next().is_some() {
                    continue;
                }
                let owner_name = owner_name.clone();
                let Some(val) = obj.remove(&key) else {
                    continue;
                };
                match obj.get_mut(&owner_name) {
                    Some(JsonValue::Object(nested)) => {
                        nested.entry(key).or_insert(val);
                    }
                    None => {
                        let mut nested = serde_json::Map::new();
                        nested.insert(key, val);
                        obj.insert(owner_name, JsonValue::Object(nested));
                    }
                    // Owner exists but is not an object — restore the key
                    // untouched and let validation surface the mismatch.
                    Some(_) => {
                        obj.insert(key, val);
                    }
                }
            }

            // 2b. Re-nest flattened array-item fields: a single
            // `steps[]` entry's keys emitted at the case level
            // (`action` / `expectedResult` on a test case) become one
            // new element of that array.
            renest_flattened_array_items(obj, properties);

            // 2c. Coerce structured values in string-typed fields
            // (e.g. `testData` emitted as a JSON object of inputs).
            coerce_structured_strings(obj, properties);

            // 3. Missing arrays normalization: Insert [] for required array fields if absent
            if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
                for req_key_val in required {
                    let Some(key) = req_key_val.as_str() else {
                        continue;
                    };
                    if obj.contains_key(key) {
                        continue;
                    }
                    let is_array = properties
                        .get(key)
                        .and_then(|p| p.get("type"))
                        .and_then(|t| t.as_str())
                        == Some("array");
                    if is_array {
                        obj.insert(key.to_string(), JsonValue::Array(Vec::new()));
                    }
                }
            }

            // 4. Recurse into each object property
            for (schema_key, property_schema) in properties {
                if let Some(val) = obj.get_mut(schema_key) {
                    normalize_value_recursively(val, property_schema);
                }
            }
        }
        JsonValue::Array(arr) => {
            if let Some(items_schema) = schema.get("items") {
                for item in arr {
                    normalize_value_recursively(item, items_schema);
                }
            }
        }
        _ => {}
    }
}

/// Fill missing required array fields with empty arrays, recursively normalize keys that exhibit
/// casing differences, and re-nest flattened nested-object fields.
///
/// Small / non-tool-trained LLMs frequently omit object keys whose value would be an empty array,
/// emit keys with incorrect casing (`camelCase` instead of `snake_case`), flatten a nested
/// object's keys onto its parent (e.g. `location.symbol` emitted as a top-level `symbol` on a
/// defect finding), or flatten a single array element's keys onto its parent (e.g. one step's
/// `action` / `expectedResult` emitted on the test case). This function normalizes all four
/// recursively.
pub(crate) fn normalize_missing_arrays(data: &mut JsonValue, tool: &ToolSchema) {
    normalize_value_recursively(data, &tool.parameters_schema);
}

fn validate_tool_output(tool: &ToolSchema, data: &JsonValue) -> AppResult<()> {
    let validator = jsonschema::JSONSchema::compile(&tool.parameters_schema).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "tool schema for `{}` is not valid JSON Schema: {e}",
            tool.name
        ))
    })?;
    let errors: Vec<String> = validator
        .validate(data)
        .err()
        .map(|errs| errs.map(|e| e.to_string()).collect())
        .unwrap_or_default();
    if !errors.is_empty() {
        let preview: String = errors.into_iter().take(3).collect::<Vec<_>>().join("; ");
        return Err(AppError::InvalidInput(format!(
            "model output for `{}` failed JSON-Schema validation: {preview}",
            tool.name
        )));
    }
    Ok(())
}

fn derive_title(request: &GenerationRequest, data: &JsonValue) -> String {
    if let Some(title) = data.get("title").and_then(|v| v.as_str()) {
        if !title.trim().is_empty() {
            return title.to_string();
        }
    }
    let kind = match request.artifact_type {
        ArtifactType::ContextMd => "Project context",
        ArtifactType::TestPlan => "Test plan",
        ArtifactType::TestCases => "Test cases",
        ArtifactType::DefectReport => "Defect report",
        ArtifactType::BugReport => "Bug report",
    };
    if request.scope_hint.trim().is_empty() {
        kind.to_string()
    } else {
        format!("{kind} — {}", request.scope_hint)
    }
}

fn render_markdown(kind: ArtifactType, data: &JsonValue) -> String {
    // Minimal Markdown renderer — the FE will eventually own a richer
    // template, but Phase 5 needs *some* human-readable representation
    // so the review queue + export-to-Markdown command both work.
    use std::fmt::Write as _;

    let mut out = String::new();
    let label = match kind {
        ArtifactType::ContextMd => "Project Context",
        ArtifactType::TestPlan => "Test Plan",
        ArtifactType::TestCases => "Test Cases",
        ArtifactType::DefectReport => "Defect Report",
        ArtifactType::BugReport => "Bug Report",
    };
    writeln!(out, "# {label}\n").expect("write");

    // Pretty-print the JSON beneath. Downstream consumers parse the
    // `structured_data` directly; this is just the human-friendly view.
    let pretty = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());
    writeln!(out, "```json\n{pretty}\n```").expect("write");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    // v1 prompt modules are kept for replay/back-compat; the
    // version-agnostic salvage/normalize mechanics tests below still
    // exercise them alongside the live-routed v2 schemas.
    use crate::prompts::{defect_report_v1, test_plan_v1};
    use crate::providers::embeddings::EmbeddingProvider as EmbeddingProviderTrait;
    use crate::providers::llm::error::LlmError;
    use crate::providers::llm::types::{
        Chunk as LlmChunkOut, FinishReason, ProviderCapabilities, Usage,
    };
    use crate::providers::llm::{ChunkStream, LlmProvider as LlmProviderTrait};
    use async_trait::async_trait;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// Mock LLM provider that yields a scripted `Vec<Chunk>`.
    #[derive(Clone)]
    struct ScriptedLlm {
        capabilities: ProviderCapabilities,
        script: Arc<Vec<LlmChunkOut>>,
    }

    impl ScriptedLlm {
        fn new(script: Vec<LlmChunkOut>) -> Self {
            Self {
                capabilities: ProviderCapabilities {
                    supports_tools: true,
                    supports_streaming: true,
                    max_context_tokens: 32_000,
                    max_output_tokens: 4_000,
                },
                script: Arc::new(script),
            }
        }
    }

    #[async_trait]
    impl LlmProviderTrait for ScriptedLlm {
        fn name(&self) -> &'static str {
            "scripted"
        }
        fn capabilities(&self) -> &ProviderCapabilities {
            &self.capabilities
        }
        fn count_tokens(&self, text: &str) -> usize {
            approximate_token_count(text)
        }
        fn stream(&self, _request: GenerateRequest) -> ChunkStream {
            let script = self.script.clone();
            Box::pin(async_stream::stream! {
                for chunk in script.iter() {
                    yield Ok::<_, LlmError>(chunk.clone());
                }
            })
        }
    }

    /// Mock embedding provider that emits the same vector regardless
    /// of input — exact value does not matter for unit tests; the
    /// vector index is filtered by provider tag rather than content.
    #[derive(Clone)]
    struct ScriptedEmbeddings {
        dim: usize,
    }

    #[async_trait]
    impl EmbeddingProviderTrait for ScriptedEmbeddings {
        fn name(&self) -> &'static str {
            "scripted-emb"
        }
        fn dimension(&self) -> usize {
            self.dim
        }
        // Trait signature is `&str` (Ollama returns `&self.model`); the
        // mock returns a literal which clippy's
        // `unnecessary_literal_bound` lint flags. We cannot change the
        // return type to `&'static str` without violating the trait.
        #[allow(clippy::unnecessary_literal_bound)]
        fn model_id(&self) -> &str {
            "test-model"
        }
        async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, LlmError> {
            Ok(inputs.into_iter().map(|_| vec![1.0; self.dim]).collect())
        }
    }

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-gen-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed project");
        (pool, path)
    }

    fn valid_test_plan_json() -> &'static str {
        r#"{
            "summary": "Plan to verify the auth subsystem covers happy and failure paths.",
            "objectives": ["Verify login", "Verify logout"],
            "scope": {
                "inScope": ["auth module"],
                "outOfScope": ["database migrations"]
            },
            "strategy": "Use a risk-based mix of API and service-level checks focused on login, logout, and session lifecycle behavior.",
            "testLevels": ["unit", "integration"],
            "testTypes": ["functional", "security"],
            "environments": ["local Express server with JSON requests"],
            "risks": [
                {"description": "Session tokens may remain active after logout.", "mitigation": "Verify revocation and post-logout access denial."}
            ],
            "entryCriteria": ["Code merged"],
            "exitCriteria": ["All tests pass"],
            "suspensionCriteria": ["Auth environment unavailable"],
            "deliverables": ["Test case suite", "Run report"]
        }"#
    }

    fn done_chunk(input: u32, output: u32) -> LlmChunkOut {
        LlmChunkOut::Done {
            usage: Usage {
                input_tokens: input,
                output_tokens: output,
            },
            finish_reason: FinishReason::Stop,
        }
    }

    fn args_chunk(s: &str) -> LlmChunkOut {
        LlmChunkOut::ToolCallArgsDelta {
            id: "call_1".into(),
            json_fragment: s.into(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generate_persists_artifact_with_metadata() {
        let (pool, path) = seed_pool().await;
        let llm = Arc::new(ScriptedLlm::new(vec![
            args_chunk(valid_test_plan_json()),
            done_chunk(120, 80),
        ]));
        let embeddings: Arc<dyn EmbeddingProviderTrait> = Arc::new(ScriptedEmbeddings { dim: 8 });

        let req = GenerationRequest {
            project_id: "p1".into(),
            project_name: "demo".into(),
            artifact_type: ArtifactType::TestPlan,
            model: "qwen2.5-coder:7b".into(),
            scope_hint: String::new(),
            project_summary: "Existing summary.".into(),
            reviewer_feedback: String::new(),
            parent_id: None,
        };

        let outcome = generate(
            req,
            &GenerationDeps {
                pool: &pool,
                llm: llm.clone(),
                embeddings: embeddings.clone(),
            },
            None,
        )
        .await
        .expect("generation succeeds");

        assert_eq!(outcome.artifact_type, ArtifactType::TestPlan);
        assert_eq!(outcome.usage_input_tokens, 120);
        assert_eq!(outcome.usage_output_tokens, 80);
        assert_eq!(outcome.structured_data["objectives"][0], "Verify login");

        let stored = artifact_repo::fetch(&pool, &outcome.artifact_id)
            .await
            .expect("fetch");
        assert_eq!(stored.generation_metadata.provider, "scripted");
        assert_eq!(stored.generation_metadata.prompt_version, "test_plan_v2");
        assert_eq!(stored.generation_metadata.input_tokens, 120);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generate_rejects_empty_project_id() {
        let (pool, path) = seed_pool().await;
        let llm: Arc<dyn LlmProviderTrait> = Arc::new(ScriptedLlm::new(vec![]));
        let embeddings: Arc<dyn EmbeddingProviderTrait> = Arc::new(ScriptedEmbeddings { dim: 8 });

        let req = GenerationRequest {
            project_id: "  ".into(),
            project_name: "demo".into(),
            artifact_type: ArtifactType::TestPlan,
            model: "qwen2.5-coder:7b".into(),
            scope_hint: String::new(),
            project_summary: String::new(),
            reviewer_feedback: String::new(),
            parent_id: None,
        };

        let err = generate(
            req,
            &GenerationDeps {
                pool: &pool,
                llm,
                embeddings,
            },
            None,
        )
        .await
        .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generate_rejects_when_tool_output_invalid_per_schema() {
        let (pool, path) = seed_pool().await;
        // Tool schema requires non-empty objectives etc.; we provide
        // an empty object.
        let bad_args = "{}";
        let llm: Arc<dyn LlmProviderTrait> = Arc::new(ScriptedLlm::new(vec![
            args_chunk(bad_args),
            done_chunk(10, 5),
        ]));
        let embeddings: Arc<dyn EmbeddingProviderTrait> = Arc::new(ScriptedEmbeddings { dim: 8 });

        let req = GenerationRequest {
            project_id: "p1".into(),
            project_name: "demo".into(),
            artifact_type: ArtifactType::TestPlan,
            model: "qwen2.5-coder:7b".into(),
            scope_hint: String::new(),
            project_summary: "S".into(),
            reviewer_feedback: String::new(),
            parent_id: None,
        };

        let err = generate(
            req,
            &GenerationDeps {
                pool: &pool,
                llm,
                embeddings,
            },
            None,
        )
        .await
        .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generate_rejects_when_no_tool_call_emitted() {
        let (pool, path) = seed_pool().await;
        // Free-text only; no ToolCallArgsDelta in script.
        let llm: Arc<dyn LlmProviderTrait> = Arc::new(ScriptedLlm::new(vec![
            LlmChunkOut::TextDelta("free-form prose".into()),
            done_chunk(10, 5),
        ]));
        let embeddings: Arc<dyn EmbeddingProviderTrait> = Arc::new(ScriptedEmbeddings { dim: 8 });

        let req = GenerationRequest {
            project_id: "p1".into(),
            project_name: "demo".into(),
            artifact_type: ArtifactType::TestPlan,
            model: "qwen2.5-coder:7b".into(),
            scope_hint: String::new(),
            project_summary: "S".into(),
            reviewer_feedback: String::new(),
            parent_id: None,
        };

        let err = generate(
            req,
            &GenerationDeps {
                pool: &pool,
                llm,
                embeddings,
            },
            None,
        )
        .await
        .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generate_rejects_oversize_prompt_via_token_budget() {
        let (pool, path) = seed_pool().await;

        let mut tiny_llm = ScriptedLlm::new(vec![]);
        // Force a very small context window so any prompt over-runs.
        tiny_llm.capabilities.max_context_tokens = 100;
        let llm: Arc<dyn LlmProviderTrait> = Arc::new(tiny_llm);
        let embeddings: Arc<dyn EmbeddingProviderTrait> = Arc::new(ScriptedEmbeddings { dim: 8 });

        let huge_summary = "x".repeat(200_000);
        let req = GenerationRequest {
            project_id: "p1".into(),
            project_name: "demo".into(),
            artifact_type: ArtifactType::TestPlan,
            model: "tiny".into(),
            scope_hint: String::new(),
            project_summary: huge_summary,
            reviewer_feedback: String::new(),
            parent_id: None,
        };

        let err = generate(
            req,
            &GenerationDeps {
                pool: &pool,
                llm,
                embeddings,
            },
            None,
        )
        .await
        .expect_err("must reject");
        assert_eq!(err.code(), "LIMIT_EXCEEDED");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn streaming_sink_receives_args_and_done_events() {
        let (pool, path) = seed_pool().await;
        let llm: Arc<dyn LlmProviderTrait> = Arc::new(ScriptedLlm::new(vec![
            args_chunk(valid_test_plan_json()),
            done_chunk(33, 22),
        ]));
        let embeddings: Arc<dyn EmbeddingProviderTrait> = Arc::new(ScriptedEmbeddings { dim: 8 });

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink_captured = captured.clone();
        let sink: StreamSink = Box::new(move |ev| {
            let label = match ev {
                StreamEvent::Text(_) => "text".to_string(),
                StreamEvent::ToolArgsDelta(_) => "args".to_string(),
                StreamEvent::Done { .. } => "done".to_string(),
            };
            sink_captured
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(label);
        });

        let req = GenerationRequest {
            project_id: "p1".into(),
            project_name: "demo".into(),
            artifact_type: ArtifactType::TestPlan,
            model: "qwen2.5-coder:7b".into(),
            scope_hint: String::new(),
            project_summary: "S".into(),
            reviewer_feedback: String::new(),
            parent_id: None,
        };

        generate(
            req,
            &GenerationDeps {
                pool: &pool,
                llm,
                embeddings,
            },
            Some(sink),
        )
        .await
        .expect("generation");

        let events = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(events.contains(&"args".to_string()));
        assert!(events.contains(&"done".to_string()));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn estimate_prompt_tokens_sums_text_blocks() {
        let messages = vec![
            crate::prompts::system_text("a".repeat(100)),
            crate::prompts::user_text("b".repeat(200)),
        ];
        let total = estimate_prompt_tokens(&messages);
        // ~75 tokens (300 chars / 4); allow some slack.
        assert!((70..=80).contains(&total), "got {total}");
    }

    #[test]
    fn validate_tool_output_accepts_valid_json() {
        let schema = test_plan_v2::tool();
        let v: JsonValue = serde_json::from_str(valid_test_plan_json()).expect("parse");
        validate_tool_output(&schema, &v).expect("valid");
    }

    #[test]
    fn validate_tool_output_rejects_missing_required_field() {
        let schema = test_plan_v2::tool();
        let v = serde_json::json!({ "summary": "ok" });
        let err = validate_tool_output(&schema, &v).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn salvage_json_returns_none_for_empty_text() {
        assert!(salvage_json_from_text("").is_none());
        assert!(salvage_json_from_text("no braces here").is_none());
    }

    #[test]
    fn salvage_json_extracts_bare_object() {
        let text = "{\"summary\":\"hello\"}";
        assert_eq!(
            salvage_json_from_text(text).as_deref(),
            Some("{\"summary\":\"hello\"}"),
        );
    }

    #[test]
    fn salvage_json_strips_markdown_fence_and_prose() {
        let text =
            "Here is the test plan:\n```json\n{\"summary\":\"ok\",\"goals\":[]}\n```\nAll done.";
        assert_eq!(
            salvage_json_from_text(text).as_deref(),
            Some("{\"summary\":\"ok\",\"goals\":[]}"),
        );
    }

    #[test]
    fn salvage_json_handles_braces_inside_strings() {
        // The naive depth counter would close after the `}` in the
        // string. The state machine ignores braces inside JSON
        // strings, so we get the full object back.
        let text = "{\"note\":\"contains } brace\",\"k\":\"v\"}";
        assert_eq!(
            salvage_json_from_text(text).as_deref(),
            Some("{\"note\":\"contains } brace\",\"k\":\"v\"}"),
        );
    }

    #[test]
    fn salvage_json_handles_nested_objects() {
        let text = "preamble {\"outer\":{\"inner\":{\"deep\":1}}} trailing";
        assert_eq!(
            salvage_json_from_text(text).as_deref(),
            Some("{\"outer\":{\"inner\":{\"deep\":1}}}"),
        );
    }

    #[test]
    fn salvage_json_handles_think_blocks() {
        let text = "<think>\nThinking about test cases...\nFor example, we might return:\n{\n  \"summary\": \"example\"\n}\n</think>\nHere is the real output:\n{\n  \"summary\": \"real summary\"\n}";
        assert_eq!(
            salvage_json_from_text(text).as_deref(),
            Some("{\n  \"summary\": \"real summary\"\n}"),
        );
    }

    #[test]
    fn salvage_json_handles_think_blocks_case_insensitive_and_unicode() {
        let text = "🦀 <Think>\nThinking... with 🦀 and İ \n</THINK> {\n  \"summary\": \"real summary with 🦀 and İ\"\n}";
        assert_eq!(
            salvage_json_from_text(text).as_deref(),
            Some("{\n  \"summary\": \"real summary with 🦀 and İ\"\n}"),
        );
    }

    #[test]
    fn salvage_tool_args_returns_bare_payload_unchanged() {
        // Direct payload — no wrapper, return as-is.
        let text = "{\"summary\":\"plan\",\"cases\":[]}";
        let got = salvage_tool_args(text, "emit_test_plan").expect("salvage");
        let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(parsed["summary"], "plan");
    }

    #[test]
    fn salvage_tool_args_unwraps_matching_tool_call_wrapper() {
        // Model emitted `{"name": "<tool>", "arguments": {...}}` —
        // unwrap to the inner arguments object.
        let text = "Here is the call:\n```json\n\
            {\"name\":\"emit_test_plan\",\"arguments\":{\"summary\":\"ok\",\"cases\":[]}}\n\
            ```";
        let got = salvage_tool_args(text, "emit_test_plan").expect("salvage");
        let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(parsed["summary"], "ok");
        assert!(parsed.get("name").is_none());
    }

    #[test]
    fn salvage_tool_args_unwraps_alternative_keys() {
        // Model emitted `{"function_name": "<tool>", "arguments": {...}}`
        let text = "{\"function_name\":\"emit_project_context\",\"arguments\":{\"summary\":\"wrapped summary\",\"architecture_notes\":\"notes\"}}";
        let got = salvage_tool_args(text, "emit_project_context").expect("salvage");
        let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(parsed["summary"], "wrapped summary");
        assert_eq!(parsed["architecture_notes"], "notes");

        // Model emitted `{"function": "<tool>", "args": {...}}`
        let text2 = "{\"function\":\"emit_test_plan\",\"args\":{\"summary\":\"wrapped summary 2\",\"cases\":[]}}";
        let got2 = salvage_tool_args(text2, "emit_test_plan").expect("salvage");
        let parsed2: serde_json::Value = serde_json::from_str(&got2).unwrap();
        assert_eq!(parsed2["summary"], "wrapped summary 2");

        // Model emitted `{"function_name": "<tool>", "parameters": {...}}`
        let text3 = "{\"function_name\":\"emit_project_context\",\"parameters\":{\"summary\":\"wrapped summary 3\",\"architecture_notes\":\"notes 3\"}}";
        let got3 = salvage_tool_args(text3, "emit_project_context").expect("salvage");
        let parsed3: serde_json::Value = serde_json::from_str(&got3).unwrap();
        assert_eq!(parsed3["summary"], "wrapped summary 3");
        assert_eq!(parsed3["architecture_notes"], "notes 3");
    }

    #[test]
    fn salvage_tool_args_recovers_gemma_project_context_tool_code() {
        let text = r#"<tool_code>
            console.log(google.admin.project_context.set_project_context({
                project_name: "Project Context",
                project_description: "A comprehensive context object for the entire application",
                category: "desktop app",
                key_modules: [
                    { name: "src-tauri", responsibility: "Backend command and provider orchestration" },
                ],
            }))
        </tool_code>"#;

        let got = salvage_tool_args(text, "emit_project_context").expect("salvage");
        let mut parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        normalize_missing_arrays(&mut parsed, &context_md_v1::tool());
        validate_tool_output(&context_md_v1::tool(), &parsed).expect("valid context payload");

        assert!(parsed["summary"]
            .as_str()
            .unwrap()
            .contains("Project Context"));
        assert!(parsed["summary"]
            .as_str()
            .unwrap()
            .contains("comprehensive context object"));
        assert_eq!(parsed["key_modules"][0]["name"], "src-tauri");
    }

    #[test]
    fn salvage_tool_args_recovers_single_quoted_tool_code_strings() {
        let text = r"<tool_code>
            console.log(default_api.emit_test_plan({
                summary: 'It\'s a plan with a } brace in prose',
                strategy: 'Verify parser resilience',
            }))
        </tool_code>";

        let got = salvage_tool_args(text, "emit_test_plan").expect("salvage");
        let mut parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        normalize_missing_arrays(&mut parsed, &test_plan_v1::tool());
        validate_tool_output(&test_plan_v1::tool(), &parsed).expect("valid test plan");
        assert_eq!(parsed["summary"], "It's a plan with a } brace in prose");
    }

    #[test]
    fn salvage_tool_args_recovers_escaped_apostrophe_in_double_quoted_strings() {
        // `\'` is a legal (if unnecessary) escape in double-quoted JS
        // strings but invalid JSON — the normalizer must drop the backslash.
        let text = r#"<tool_code>
            console.log(default_api.emit_test_plan({
                summary: "It\'s a plan",
                strategy: "Verify parser resilience",
            }))
        </tool_code>"#;

        let got = salvage_tool_args(text, "emit_test_plan").expect("salvage");
        let mut parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        normalize_missing_arrays(&mut parsed, &test_plan_v1::tool());
        validate_tool_output(&test_plan_v1::tool(), &parsed).expect("valid test plan");
        assert_eq!(parsed["summary"], "It's a plan");
    }

    #[test]
    fn normalize_js_strings_drops_escaped_single_quote_backslash() {
        // `\'` is a legal JS escape but not a legal JSON escape — the
        // backslash must be dropped, in both quote styles.
        assert_eq!(
            normalize_js_strings(r"{summary: 'it\'s a test'}"),
            r#"{summary: "it's a test"}"#
        );
        assert_eq!(
            normalize_js_strings(r#"{summary: "it\'s a test"}"#),
            r#"{summary: "it's a test"}"#
        );
        // `\"` and `\\` are valid JSON escapes and must survive untouched.
        assert_eq!(
            normalize_js_strings(r#"{summary: "say \"hi\" \\ there"}"#),
            r#"{summary: "say \"hi\" \\ there"}"#
        );
    }

    #[test]
    fn salvage_tool_args_recovers_escaped_single_quotes_in_js_strings() {
        let text = r"<tool_code>
            console.log(default_api.emit_test_plan({
                summary: 'it\'s a test of the parser\'s escapes',
                strategy: 'Verify salvage handles \' sequences',
            }))
        </tool_code>";

        let got = salvage_tool_args(text, "emit_test_plan").expect("salvage");
        let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(parsed["summary"], "it's a test of the parser's escapes");
        assert_eq!(parsed["strategy"], "Verify salvage handles ' sequences");
    }

    #[test]
    fn salvage_tool_args_recovers_pseudo_calls_for_artifact_arrays() {
        let bug_text = r#"```tool_code
            console.log(default_api.emit_bug_report({
                bugs: [{
                    id: "BUG-RUNTIME",
                    title: "Runtime failure when saving reports",
                    severity: "major",
                    priority: "p1",
                    reproducibility: "always",
                    stepsToReproduce: ["1. Open the app"],
                    expectedBehavior: "The report is saved",
                    actualBehavior: "The report fails",
                    rootCause: { symbol: "saveReport", explanation: "Error is not handled" },
                }],
            }))
        ```"#;
        let bug = salvage_tool_args(bug_text, "emit_bug_report").expect("salvage bug report");
        let mut bug_value: serde_json::Value = serde_json::from_str(&bug).unwrap();
        normalize_missing_arrays(&mut bug_value, &bug_report_v2::tool());
        validate_tool_output(&bug_report_v2::tool(), &bug_value).expect("valid bug report");

        let cases_text = r#"<tool_code>
            console.log(default_api.emit_test_cases({
                cases: [{
                    id: "TC-SAVE-REPORT",
                    title: "Save report round-trip",
                    type: "positive",
                    priority: "p1",
                    steps: [{ action: "Save a report", expectedResult: "The report is persisted" }],
                    traceability: "src/report.ts#saveReport",
                }],
            }))
        </tool_code>"#;
        let cases = salvage_tool_args(cases_text, "emit_test_cases").expect("salvage cases");
        let mut cases_value: serde_json::Value = serde_json::from_str(&cases).unwrap();
        normalize_missing_arrays(&mut cases_value, &test_cases_v2::tool());
        validate_tool_output(&test_cases_v2::tool(), &cases_value).expect("valid test cases");
        assert_eq!(
            cases_value["cases"][0]["traceability"],
            serde_json::json!(["src/report.ts#saveReport"])
        );
    }

    #[test]
    fn salvage_tool_args_recovers_pseudo_calls_for_plan_and_defects() {
        let plan_text = r#"<tool_code>
            console.log(default_api.emit_test_plan({
                summary: "Focused plan for report workflows",
                objectives: ["Verify report creation"],
                scopeIn: ["src/report.ts"],
                scopeOut: [],
                strategy: "Exercise unit and integration behavior around reports.",
                environments: ["vitest"],
                risks: [{ description: "Persistence can fail", mitigation: "Mock failures" }],
                entryCriteria: ["Source is indexed"],
                exitCriteria: ["Critical report cases pass"],
            }))
        </tool_code>"#;
        let plan = salvage_tool_args(plan_text, "emit_test_plan").expect("salvage plan");
        let mut plan_value: serde_json::Value = serde_json::from_str(&plan).unwrap();
        normalize_missing_arrays(&mut plan_value, &test_plan_v2::tool());
        validate_tool_output(&test_plan_v2::tool(), &plan_value).expect("valid test plan");
        // Flat v1-style scope keys are re-nested into the v2 `scope` object.
        assert_eq!(
            plan_value["scope"]["inScope"],
            serde_json::json!(["src/report.ts"])
        );
        assert_eq!(plan_value["scope"]["outOfScope"], serde_json::json!([]));
        assert!(plan_value.get("scopeIn").is_none());
        assert!(plan_value.get("scopeOut").is_none());

        let defect_text = r#"```tool_code
            console.log(default_api.emit_defect_report({
                findings: [{
                    id: "DEF-UNHANDLED-ERROR",
                    severity: "major",
                    category: "error_handling",
                    confidence: "high",
                    location: { symbol: "saveReport", start_line: 10, end_line: 20 },
                    description: "The save path does not handle provider errors.",
                    impact: "Users can lose generated report data.",
                    suggested_fix: "Catch the provider error and surface a retryable failure.",
                }],
                summary: "One high-confidence defect found.",
            }))
        ```"#;
        let defect = salvage_tool_args(defect_text, "emit_defect_report").expect("salvage defect");
        let mut defect_value: serde_json::Value = serde_json::from_str(&defect).unwrap();
        normalize_missing_arrays(&mut defect_value, &defect_report_v1::tool());
        validate_tool_output(&defect_report_v1::tool(), &defect_value)
            .expect("valid defect report");
    }

    #[test]
    fn salvage_remap_rebuild_produces_v2_test_plan_shape() {
        // Free-text JSON missing `summary`/`strategy` triggers the rebuild
        // path. The rebuilt object must satisfy the v2 schema: nested
        // `scope` instead of flat `scopeIn`/`scopeOut`, with the remaining
        // required arrays backfilled by normalization.
        let text = r#"{"title":"Report flows","scopeIn":["src/report.ts"],"scopeOut":[],"objectives":["Verify report creation"]}"#;
        let got = salvage_tool_args(text, "emit_test_plan").expect("salvage");
        let mut value: serde_json::Value = serde_json::from_str(&got).unwrap();
        normalize_missing_arrays(&mut value, &test_plan_v2::tool());
        validate_tool_output(&test_plan_v2::tool(), &value)
            .expect("rebuilt plan validates against v2");
        assert_eq!(
            value["scope"]["inScope"],
            serde_json::json!(["src/report.ts"])
        );
        assert!(value.get("scopeIn").is_none());
        assert!(value.get("scopeOut").is_none());

        // Rebuild with no scope information at all still emits the
        // required `scope` object shell.
        let bare = r#"{"title":"Bare plan"}"#;
        let got_bare = salvage_tool_args(bare, "emit_test_plan").expect("salvage bare");
        let mut bare_value: serde_json::Value = serde_json::from_str(&got_bare).unwrap();
        normalize_missing_arrays(&mut bare_value, &test_plan_v2::tool());
        validate_tool_output(&test_plan_v2::tool(), &bare_value)
            .expect("bare rebuilt plan validates against v2");
        assert_eq!(bare_value["scope"]["inScope"], serde_json::json!([]));
        assert_eq!(bare_value["scope"]["outOfScope"], serde_json::json!([]));
    }

    #[test]
    fn detect_non_tool_call_format_flags_native_formats() {
        assert_eq!(
            detect_non_tool_call_format("<tool_code> console.log(set_project_context())"),
            Some("Gemma-style `tool_code` blocks")
        );
        assert_eq!(
            detect_non_tool_call_format("<tool_call>{}</tool_call>"),
            Some("`<tool_call>` tags")
        );
        assert_eq!(
            detect_non_tool_call_format("<|python_tag|>emit(...)"),
            Some("Llama function-call tags")
        );
        assert_eq!(
            detect_non_tool_call_format("print(default_api.foo())"),
            Some("a code snippet")
        );
        assert_eq!(
            detect_non_tool_call_format("You could use console.log(result) to debug this."),
            None
        );
        assert_eq!(
            detect_non_tool_call_format("Try print(result) while investigating."),
            None
        );
        assert_eq!(
            detect_non_tool_call_format("I cannot help with that."),
            None
        );
    }

    #[test]
    fn extract_raw_json_reports_tool_incapable_model() {
        let schema = ToolSchema {
            name: "emit_project_context".into(),
            description: String::new(),
            parameters_schema: serde_json::json!({}),
        };
        let aggregate = StreamAggregate {
            tool_args: String::new(),
            text: "<tool_code> console.log(set_project_context())".into(),
            text_len: 60,
            input_tokens: 0,
            output_tokens: 0,
        };
        let err = extract_raw_json(&aggregate, &schema, "gemma3n:e4b")
            .expect_err("tool-incapable output must error");
        let msg = err.to_string();
        assert!(msg.contains("did not invoke `emit_project_context`"), "got: {msg}");
        assert!(
            msg.contains("tool_code"),
            "must name the detected format: {msg}"
        );
    }

    #[test]
    fn extract_raw_json_salvages_tool_code_before_erroring() {
        let schema = context_md_v1::tool();
        let aggregate = StreamAggregate {
            tool_args: String::new(),
            text: "<tool_code> console.log(set_project_context({ project_name: \"X\", project_description: \"Y\" }))".into(),
            text_len: 96,
            input_tokens: 0,
            output_tokens: 0,
        };

        let raw = extract_raw_json(&aggregate, &schema, "gemma3n:e4b").expect("salvage");
        let mut parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        normalize_missing_arrays(&mut parsed, &schema);
        validate_tool_output(&schema, &parsed).expect("valid context payload");
        assert!(parsed["summary"].as_str().unwrap().contains('X'));
    }

    #[test]
    fn salvage_tool_args_unwraps_custom_payload_keys_and_inline_wrappers() {
        // Model emitted `{"tool": "emit_bug_report", "report": {"bugs": []}}`
        let text = "{\"tool\":\"emit_bug_report\",\"report\":{\"bugs\":[]}}";
        let got = salvage_tool_args(text, "emit_bug_report").expect("salvage");
        let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(parsed["bugs"], serde_json::json!([]));

        // Model emitted inline wrapper: `{"tool": "emit_project_context", "summary": "A", "architecture_notes": "B", "key_modules": []}`
        let text2 = "{\"tool\":\"emit_project_context\",\"summary\":\"A\",\"architecture_notes\":\"B\",\"key_modules\":[]}";
        let got2 = salvage_tool_args(text2, "emit_project_context").expect("salvage");
        let parsed2: serde_json::Value = serde_json::from_str(&got2).unwrap();
        assert_eq!(parsed2["summary"], "A");
        assert_eq!(parsed2["architecture_notes"], "B");
        assert!(parsed2.get("tool").is_none());

        // Model emitted inline wrapper with array field `bugs`: `{"tool": "emit_bug_report", "bugs": []}`
        let text3 = "{\"tool\":\"emit_bug_report\",\"bugs\":[]}";
        let got3 = salvage_tool_args(text3, "emit_bug_report").expect("salvage");
        let parsed3: serde_json::Value = serde_json::from_str(&got3).unwrap();
        assert_eq!(parsed3["bugs"], serde_json::json!([]));
        assert!(parsed3.get("tool").is_none());
    }

    #[test]
    fn salvage_tool_args_rejects_per_item_wrapper() {
        // The model wrapped a single case row inside a tool-call
        // shell that names the case id, not the tool. We cannot
        // recover the full `cases` array from one row, so we return
        // None and let the caller surface a clear error.
        let text = "```json\n\
            {\"name\":\"TC-1\",\"arguments\":{\"title\":\"x\"}}\n\
            ```";
        assert!(salvage_tool_args(text, "emit_test_cases").is_none());
    }

    #[test]
    fn salvage_tool_args_returns_none_for_no_json() {
        assert!(salvage_tool_args("just prose", "emit_test_plan").is_none());
    }

    #[test]
    fn salvage_tool_args_remaps_codebase_dumps_and_bare_arrays() {
        // Test case 1: codebase dump mapping for emit_project_context
        let dump =
            r#"{"architect":"Yuvraj","project_title":"Tessera","description":"Beautiful app"}"#;
        let got = salvage_tool_args(dump, "emit_project_context").expect("salvage");
        let parsed: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert!(parsed["summary"].as_str().unwrap().contains("Tessera"));
        assert!(parsed["summary"]
            .as_str()
            .unwrap()
            .contains("Beautiful app"));
        assert!(parsed["architecture_notes"]
            .as_str()
            .unwrap()
            .contains("architect: Yuvraj"));

        // Test case 2: bare array wrapping for emit_test_cases
        let array_str = r#"[{"id":"TC-1","title":"test_login"}]"#;
        let got2 = salvage_tool_args(array_str, "emit_test_cases").expect("salvage");
        let parsed2: serde_json::Value = serde_json::from_str(&got2).unwrap();
        assert_eq!(parsed2["cases"][0]["id"], "TC-1");
        assert_eq!(parsed2["cases"][0]["title"], "test_login");

        // Test case 3: single object wrapping for emit_test_cases and traceability object/string conversion
        let single_str = r#"{"id":"TC-2","title":"test_logout","steps":[],"traceability":{"file_hint":"/src/Navbar.js","symbol":"Navbar"}}"#;
        let got3 = salvage_tool_args(single_str, "emit_test_cases").expect("salvage");
        let parsed3: serde_json::Value = serde_json::from_str(&got3).unwrap();
        assert_eq!(parsed3["cases"][0]["id"], "TC-2");
        assert_eq!(parsed3["cases"][0]["title"], "test_logout");
        assert_eq!(
            parsed3["cases"][0]["traceability"],
            serde_json::json!(["/src/Navbar.js#Navbar"])
        );

        // Test case 4: traceability as a single string is wrapped in an array
        let single_str_trace = r#"{"cases":[{"id":"TC-3","title":"test_nav","steps":[],"traceability":"/src/Navbar.js#Navbar"}]}"#;
        let got4 = salvage_tool_args(single_str_trace, "emit_test_cases").expect("salvage");
        let parsed4: serde_json::Value = serde_json::from_str(&got4).unwrap();
        assert_eq!(
            parsed4["cases"][0]["traceability"],
            serde_json::json!(["/src/Navbar.js#Navbar"])
        );
    }

    #[test]
    fn normalize_missing_arrays_fills_absent_array_fields() {
        let schema = test_plan_v1::tool();
        // Model omitted `objectives` and `environments` entirely.
        let mut data = serde_json::json!({
            "summary": "short",
            "scopeIn": ["auth"],
            "scopeOut": [],
            "strategy": "risk-based",
            "risks": [],
            "entryCriteria": ["code merged"],
            "exitCriteria": ["all pass"]
        });
        normalize_missing_arrays(&mut data, &schema);
        assert_eq!(data["objectives"], serde_json::json!([]));
        assert_eq!(data["environments"], serde_json::json!([]));
        // Already-present fields remain untouched.
        assert_eq!(data["scopeIn"], serde_json::json!(["auth"]));
    }

    #[test]
    fn normalize_missing_arrays_ignores_non_array_required_fields() {
        let schema = test_plan_v1::tool();
        // Omit `summary` (a string, not array) — should NOT be filled.
        let mut data = serde_json::json!({
            "objectives": ["verify login"],
            "scopeIn": ["auth"],
            "scopeOut": [],
            "strategy": "risk-based",
            "environments": [],
            "risks": [],
            "entryCriteria": ["code merged"],
            "exitCriteria": ["all pass"]
        });
        normalize_missing_arrays(&mut data, &schema);
        assert!(data.get("summary").is_none());
    }

    #[test]
    fn normalize_missing_arrays_normalizes_casing_recursively() {
        let schema = context_md_v1::tool();
        let mut data = serde_json::json!({
            "summary": "elevator pitch",
            "architectureNotes": "some architecture notes here",
            "keyModules": [
                {
                    "name": "auth",
                    "Responsibility": "handles users"
                }
            ]
        });
        normalize_missing_arrays(&mut data, &schema);
        assert_eq!(data["architecture_notes"], "some architecture notes here");
        assert!(data.get("architectureNotes").is_none());
        assert_eq!(data["key_modules"][0]["responsibility"], "handles users");
        assert!(data["key_modules"][0].get("Responsibility").is_none());
        assert_eq!(data["key_modules"][0]["name"], "auth");
    }

    #[test]
    fn normalize_missing_arrays_handles_camelcase_array_renaming_and_filling() {
        let schema = test_plan_v1::tool();
        let mut data = serde_json::json!({
            "summary": "short",
            "objectives": ["verify login"],
            "scopeIn": ["auth"],
            "scopeOut": [],
            "strategy": "risk-based",
            "environments": [],
            "risks": [],
            "entry_criteria": ["code merged"] // snake_case instead of camelCase entryCriteria
            // exitCriteria is omitted completely
        });
        normalize_missing_arrays(&mut data, &schema);
        assert_eq!(data["entryCriteria"], serde_json::json!(["code merged"]));
        assert!(data.get("entry_criteria").is_none());
        assert_eq!(data["exitCriteria"], serde_json::json!([]));
    }

    #[test]
    fn normalize_renests_flattened_defect_location_fields() {
        // Reproduces the live failure: the model emitted the `location`
        // object's keys at the finding level, which
        // `additionalProperties: false` rejected with "Additional
        // properties are not allowed ('end_line', 'file_hint',
        // 'start_line', 'symbol' were unexpected)".
        let schema = defect_report_v1::tool();
        let mut data = serde_json::json!({
            "findings": [{
                "id": "DEF-UNHANDLED-ERROR",
                "severity": "major",
                "category": "error_handling",
                "confidence": "high",
                "symbol": "saveReport",
                "start_line": 10,
                "end_line": 20,
                "file_hint": "src/report.ts",
                "description": "The save path does not handle provider errors.",
                "impact": "Users can lose generated report data.",
                "suggested_fix": "Catch the provider error and surface a retryable failure."
            }],
            "summary": "One high-confidence defect found."
        });
        normalize_missing_arrays(&mut data, &schema);
        validate_tool_output(&schema, &data).expect("flattened location heals to valid");
        let finding = &data["findings"][0];
        assert_eq!(finding["location"]["symbol"], "saveReport");
        assert_eq!(finding["location"]["start_line"], 10);
        assert_eq!(finding["location"]["end_line"], 20);
        assert_eq!(finding["location"]["file_hint"], "src/report.ts");
        assert!(finding.get("symbol").is_none());
        assert!(finding.get("start_line").is_none());
    }

    #[test]
    fn normalize_renests_flattened_bug_root_cause_fields() {
        // Same flattening failure mode for bug reports: rootCause keys
        // emitted at the bug level, including a snake_case variant that
        // the casing pass then fixes after the move.
        let schema = bug_report_v2::tool();
        let mut data = serde_json::json!({
            "bugs": [{
                "id": "BUG-SAVE-RACE",
                "title": "Report save races under load",
                "severity": "major",
                "priority": "p1",
                "reproducibility": "always",
                "stepsToReproduce": ["1. Save twice quickly"],
                "expectedBehavior": "One report row is written",
                "actualBehavior": "Two rows are written",
                "symbol": "saveReport",
                "file_hint": "src/report.ts",
                "explanation": "No write lock around the insert."
            }]
        });
        normalize_missing_arrays(&mut data, &schema);
        validate_tool_output(&schema, &data).expect("flattened rootCause heals to valid");
        let bug = &data["bugs"][0];
        assert_eq!(bug["rootCause"]["symbol"], "saveReport");
        assert_eq!(bug["rootCause"]["fileHint"], "src/report.ts");
        assert_eq!(
            bug["rootCause"]["explanation"],
            "No write lock around the insert."
        );
        assert!(bug.get("symbol").is_none());
        assert!(bug.get("file_hint").is_none());
    }

    #[test]
    fn normalize_renests_flattened_test_case_step_fields() {
        // Reproduces the golden-suite failure on qwen2.5-coder:1.5b: the
        // model emitted a single step's `action` / `expectedResult` at
        // the case level instead of inside `steps[]`, which
        // `additionalProperties: false` rejected with "Additional
        // properties are not allowed ('action', 'expectedResult' were
        // unexpected)".
        let schema = test_cases_v2::tool();
        let mut data = serde_json::json!({
            "cases": [{
                "id": "TC-LOGIN-SUCCESS",
                "title": "Login succeeds with valid credentials",
                "type": "positive",
                "priority": "p1",
                "action": "Call login with valid credentials",
                "expectedResult": "A session token is returned"
            }]
        });
        normalize_missing_arrays(&mut data, &schema);
        validate_tool_output(&schema, &data).expect("flattened step heals to valid");
        let case = &data["cases"][0];
        assert_eq!(case["steps"][0]["action"], "Call login with valid credentials");
        assert_eq!(case["steps"][0]["expectedResult"], "A session token is returned");
        assert!(case.get("action").is_none());
        assert!(case.get("expectedResult").is_none());
    }

    #[test]
    fn normalize_coerces_structured_test_data_to_string() {
        // Reproduces the golden-suite failure on qwen2.5-coder:1.5b: the
        // model emitted `testData` as a JSON object of inputs
        // (`{"email": "...", "password": "..."}`) where the schema wants
        // a string — "is not of type \"string\"".
        let schema = test_cases_v2::tool();
        let mut data = serde_json::json!({
            "cases": [{
                "id": "TC-LOGIN-SUCCESS",
                "title": "Login succeeds with valid credentials",
                "type": "positive",
                "priority": "p1",
                "testData": { "email": "user@example.com", "password": "securePassword" },
                "steps": [
                    { "action": "Call login", "expectedResult": "Token returned" }
                ]
            }]
        });
        normalize_missing_arrays(&mut data, &schema);
        validate_tool_output(&schema, &data).expect("structured testData heals to valid");
        let test_data = data["cases"][0]["testData"].as_str().expect("string");
        assert!(test_data.contains("user@example.com"));
        // Plain string values are untouched by the coercion pass.
        assert_eq!(data["cases"][0]["title"], "Login succeeds with valid credentials");
    }

    #[test]
    fn normalize_array_item_renest_does_not_clobber_existing_elements() {
        // A populated steps[] array must never be mutated by the lift —
        // the stray case-level keys stay put and validation reports them.
        let schema = test_cases_v2::tool();
        let mut data = serde_json::json!({
            "cases": [{
                "id": "TC-LOGIN-SUCCESS",
                "title": "Login succeeds with valid credentials",
                "type": "positive",
                "priority": "p1",
                "steps": [
                    { "action": "Real step", "expectedResult": "Real result" }
                ],
                "action": "Stray flattened action"
            }]
        });
        normalize_missing_arrays(&mut data, &schema);
        let case = &data["cases"][0];
        assert_eq!(case["steps"].as_array().map(Vec::len), Some(1));
        assert_eq!(case["steps"][0]["action"], "Real step");
        assert_eq!(case["action"], "Stray flattened action");
        assert!(validate_tool_output(&schema, &data).is_err());
    }

    #[test]
    fn normalize_renest_does_not_clobber_existing_nested_values() {
        let schema = defect_report_v1::tool();
        let mut data = serde_json::json!({
            "findings": [{
                "id": "DEF-X",
                "severity": "major",
                "category": "logic_error",
                "confidence": "high",
                // Partial location present; stray duplicate symbol at the
                // finding level must not overwrite the nested one.
                "location": { "symbol": "realSymbol", "start_line": 1, "end_line": 2 },
                "symbol": "straySymbol",
                "description": "A description long enough.",
                "impact": "Some impact.",
                "suggested_fix": "Some fix."
            }],
            "summary": "s"
        });
        normalize_missing_arrays(&mut data, &schema);
        let finding = &data["findings"][0];
        assert_eq!(finding["location"]["symbol"], "realSymbol");
        assert!(finding.get("symbol").is_none());
    }
}
