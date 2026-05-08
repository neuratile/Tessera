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
    bug_report_v1, context_md_v1, defect_report_v1, test_cases_v1, test_plan_v1, PromptContext,
};
use crate::providers::embeddings::EmbeddingProvider;
use crate::providers::llm::types::{Chunk as LlmChunk, GenerateRequest, Message, ToolSchema};
use crate::providers::llm::{approximate_token_count, LlmProvider};
use crate::repositories::artifact_repo::{self, ArtifactInsert, ArtifactType, GenerationMetadata};
use crate::repositories::chunk_repo;
use crate::services::chunking_service::Chunk as CodeChunk;

/// Reserve at least this many tokens for the model's response so the
/// prompt cannot consume the entire context window.
pub const RESPONSE_RESERVE_TOKENS: u32 = 4_000;

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
    let ctx = PromptContext {
        project_name: &request.project_name,
        project_summary: &request.project_summary,
        chunks: &chunks,
        scope_hint: &request.scope_hint,
        reviewer_feedback: &request.reviewer_feedback,
    };
    let (messages, tool_schema, prompt_version) = build_prompt(request.artifact_type, &ctx);

    // 3. Token budget — refuse before sending the request.
    let capabilities = deps.llm.capabilities();
    let prompt_token_estimate = estimate_prompt_tokens(&messages);
    let budget = capabilities
        .max_context_tokens
        .saturating_sub(RESPONSE_RESERVE_TOKENS);
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

    if aggregated.tool_args.trim().is_empty() {
        return Err(AppError::InvalidInput(format!(
            "model did not invoke `{}` — got {} chars of free text instead",
            tool_schema.name, aggregated.text_len
        )));
    }

    // 5. Parse + validate against the JSON Schema.
    let structured_data: JsonValue =
        serde_json::from_str(&aggregated.tool_args).map_err(AppError::Serde)?;
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

/// Result of draining the LLM stream — extracted so [`generate`] stays
/// inside the clippy `too_many_lines` budget.
struct StreamAggregate {
    tool_args: String,
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
    let mut text_len: usize = 0;
    let mut input_tokens = 0_u32;
    let mut output_tokens = 0_u32;

    let mut stream = llm.stream(request);
    while let Some(item) = stream.next().await {
        match item? {
            LlmChunk::TextDelta(text) => {
                text_len = text_len.saturating_add(text.len());
                if let Some(s) = sink.as_deref_mut() {
                    s(StreamEvent::Text(text));
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
        text_len,
        input_tokens,
        output_tokens,
    })
}

async fn retrieve_chunks(
    request: &GenerationRequest,
    deps: &GenerationDeps<'_>,
) -> AppResult<Vec<CodeChunk>> {
    if request.scope_hint.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut embeddings = deps
        .embeddings
        .embed(vec![request.scope_hint.clone()])
        .await?;
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

    Ok(hits
        .into_iter()
        .filter(|h| h.similarity >= MIN_SIMILARITY)
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
            test_plan_v1::build_messages(ctx),
            test_plan_v1::tool(),
            test_plan_v1::VERSION,
        ),
        ArtifactType::TestCases => (
            test_cases_v1::build_messages(ctx),
            test_cases_v1::tool(),
            test_cases_v1::VERSION,
        ),
        ArtifactType::DefectReport => (
            defect_report_v1::build_messages(ctx),
            defect_report_v1::tool(),
            defect_report_v1::VERSION,
        ),
        ArtifactType::BugReport => (
            bug_report_v1::build_messages(ctx),
            bug_report_v1::tool(),
            bug_report_v1::VERSION,
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
            "scopeIn": ["auth module"],
            "scopeOut": [],
            "strategy": "Use a risk-based mix of API and service-level checks focused on login, logout, and session lifecycle behavior.",
            "environments": ["local Express server with JSON requests"],
            "risks": [
                {"description": "Session tokens may remain active after logout.", "mitigation": "Verify revocation and post-logout access denial."}
            ],
            "entryCriteria": ["Code merged"],
            "exitCriteria": ["All tests pass"]
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
        assert_eq!(stored.generation_metadata.prompt_version, "test_plan_v1");
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
        let schema = test_plan_v1::tool();
        let v: JsonValue = serde_json::from_str(valid_test_plan_json()).expect("parse");
        validate_tool_output(&schema, &v).expect("valid");
    }

    #[test]
    fn validate_tool_output_rejects_missing_required_field() {
        let schema = test_plan_v1::tool();
        let v = serde_json::json!({ "summary": "ok" });
        let err = validate_tool_output(&schema, &v).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }
}
