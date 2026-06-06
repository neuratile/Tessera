//! Versioned prompt templates.
//!
//! Per `rules.md` §12.1: each prompt is a typed function in its own
//! file, suffixed with a version (`_v1`, `_v2`). Never silently mutate
//! a live prompt — bump the version. All prompts produce structured
//! output via JSON Schema function-calling and are tested against the
//! lowest-capability target model (qwen2.5-coder:7b) first.
//!
//! ## Layout
//!
//! - [`context_md_v1`] — generates the per-project `context.md`
//!   summary from sampled chunks (rules.md §12.3 RAG bootstrap).
//! - [`test_plan_v1`] — full-project test plan: scope, objectives,
//!   strategy, risk matrix, entry/exit criteria.
//! - [`test_cases_v1`] — individual test cases bound to specific
//!   functions / endpoints.
//! - [`test_cases_v2`] — v1 plus `TestRail` separated steps
//!   (`{ action, expectedResult }`), case `type`, `testData`,
//!   `postconditions`; the runnable `files[]` contract is unchanged.
//! - [`test_plan_v2`] — v1 plus nested `scope`, `suspensionCriteria`,
//!   `testLevels` / `testTypes` enums, `deliverables` (29119-lite).
//! - [`defect_report_v1`] — static-analysis findings (severity,
//!   category, location, suggested fix, confidence).
//! - [`defect_report_v2`] — v1 plus CWE-aligned categories, required
//!   `fixSuggestion`, evidence parity with the bug report.
//! - [`bug_report_v1`] — runtime-issue tracking docs formatted for
//!   issue-tracker import.
//! - [`bug_report_v2`] — v1 plus severity↔priority split (5-level
//!   severity), `reproducibility`, `workaround`, `component`.
//!
//! Every prompt:
//! - Returns `Vec<Message>` ready to feed into an `LlmProvider`.
//! - Exposes a `ToolSchema` carrying the JSON-Schema function-call
//!   shape that the model is required to emit.
//! - Carries a string version constant matching the file suffix —
//!   the consumer (Phase 5 generation service) records it on every
//!   artifact for traceability.

use std::fmt::Write as _;

use crate::providers::llm::types::{Content, Message, ToolSchema};
use crate::services::chunking_service::Chunk;

pub mod bug_report_v1;
pub mod bug_report_v2;
pub mod context_md_v1;
pub mod defect_report_v1;
pub mod defect_report_v2;
pub mod test_cases_v1;
pub mod test_cases_v2;
pub mod test_plan_v1;
pub mod test_plan_v2;

#[cfg(test)]
mod snapshots;

/// Maximum number of retrieved chunks the assembler will inline into a
/// single prompt before the consumer must fall back to summarization.
/// Picked to keep the assembled prompt under ~120K tokens for the
/// smallest target model (Qwen 32K context).
pub const MAX_INLINE_CHUNKS: usize = 40;

/// Common context every prompt consumes. Producers (Phase 5
/// generation service) assemble this from the RAG search hit and the
/// project-level `context.md` snapshot.
#[derive(Debug, Clone)]
pub struct PromptContext<'a> {
    /// Project name as provided by the user (display only).
    pub project_name: &'a str,
    /// Bottom-up summarization of the project — usually the
    /// `context.md` produced by [`context_md_v1`].
    pub project_summary: &'a str,
    /// Most-relevant code chunks retrieved for this generation.
    /// Capped at [`MAX_INLINE_CHUNKS`] before being passed in.
    pub chunks: &'a [Chunk],
    /// Optional caller scope hint (`auth module`,
    /// `src/payments/checkout.ts`) — empty when the artifact spans
    /// the whole project.
    pub scope_hint: &'a str,
    /// Optional reviewer feedback from a previous regeneration cycle.
    /// Empty on first pass.
    pub reviewer_feedback: &'a str,
}

impl PromptContext<'_> {
    /// Render the chunks into a single user-facing context block.
    /// Each chunk is preceded by a stable header so the model can
    /// cite back to specific functions / classes by name.
    #[must_use]
    pub fn render_chunks(&self) -> String {
        if self.chunks.is_empty() {
            return "(no relevant code chunks retrieved)".to_string();
        }
        let mut out = String::new();
        for (i, chunk) in self.chunks.iter().take(MAX_INLINE_CHUNKS).enumerate() {
            let kind = chunk_kind_label(chunk);
            let name = if chunk.name.is_empty() {
                "<module>"
            } else {
                chunk.name.as_str()
            };
            let idx = i + 1;
            let start = chunk.start_line;
            let end = chunk.end_line;
            let content = &chunk.content;
            // `write!` to a `String` cannot fail; `expect` is acceptable
            // per `rules.md` §2.2 because the failure mode is allocator
            // exhaustion, not user input.
            writeln!(
                out,
                "--- chunk {idx} | {kind} `{name}` (lines {start}–{end}) ---"
            )
            .expect("write to String must succeed");
            writeln!(out, "{content}").expect("write to String must succeed");
            out.push('\n');
        }
        if self.chunks.len() > MAX_INLINE_CHUNKS {
            let extra = self.chunks.len() - MAX_INLINE_CHUNKS;
            writeln!(
                out,
                "(... {extra} more chunks omitted; ask for narrower scope to see them)"
            )
            .expect("write to String must succeed");
        }
        out
    }
}

fn chunk_kind_label(chunk: &Chunk) -> &'static str {
    use crate::services::chunking_service::ChunkKind;
    match chunk.kind {
        ChunkKind::Function => "function",
        ChunkKind::Method => "method",
        ChunkKind::Class => "class",
        ChunkKind::Module => "module",
    }
}

/// Convenience constructor: a single-text-block user message. Most
/// prompts use this for the populated request body and reserve the
/// system role for static instructions.
#[must_use]
pub fn user_text(text: impl Into<String>) -> Message {
    Message {
        role: crate::providers::llm::types::Role::User,
        content: vec![Content::Text { text: text.into() }],
    }
}

/// Convenience constructor: a system message (instructions /
/// guard-rails). Each prompt's system text is a constant in its own
/// file so a snapshot test catches accidental edits.
#[must_use]
pub fn system_text(text: impl Into<String>) -> Message {
    Message {
        role: crate::providers::llm::types::Role::System,
        content: vec![Content::Text { text: text.into() }],
    }
}

/// JSON-Schema for the runnable `files[]` workspace carried on a
/// test-cases artifact — the contract the sandbox runner consumes
/// (`RunInput` / `sandbox_service`). Shared by `test_cases_v1` and
/// `test_cases_v2` so the contract cannot drift between prompt
/// versions; both modules' tool snapshots lock the emitted value.
#[must_use]
pub fn runnable_files_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "array",
        "description": "Runnable workspace mirroring the cases: minimal source-under-test plus generated vitest specs, so the local sandbox can execute them. Optional — omit for descriptive-only cases.",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["path", "contents", "isTest"],
            "properties": {
                "path": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Workspace-relative path, e.g. `src/add.ts` or `add.test.ts`. No absolute paths, no `..`."
                },
                "contents": {
                    "type": "string",
                    "description": "Full file contents."
                },
                "isTest": {
                    "type": "boolean",
                    "description": "true for a generated vitest spec; false for source-under-test."
                }
            }
        }
    })
}

/// Helper: produce a `ToolSchema` from a name, description, and a
/// JSON-Schema document. The Phase 2 `ToolSchema` type stores the
/// schema as `serde_json::Value` so we wrap the literal here.
#[must_use]
pub fn tool_schema(
    name: impl Into<String>,
    description: impl Into<String>,
    parameters_schema: serde_json::Value,
) -> ToolSchema {
    ToolSchema {
        name: name.into(),
        description: description.into(),
        parameters_schema,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::chunking_service::ChunkKind;

    fn sample_chunk(name: &str, kind: ChunkKind, content: &str) -> Chunk {
        Chunk {
            kind,
            name: name.to_string(),
            start_line: 1,
            end_line: 5,
            content: content.to_string(),
            token_count: 10,
            oversize: false,
        }
    }

    #[test]
    fn render_chunks_emits_stable_header() {
        let chunks = vec![sample_chunk("greet", ChunkKind::Function, "fn greet() {}")];
        let ctx = PromptContext {
            project_name: "x",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let rendered = ctx.render_chunks();
        assert!(rendered.contains("--- chunk 1 | function `greet`"));
        assert!(rendered.contains("(lines 1–5)"));
        assert!(rendered.contains("fn greet() {}"));
    }

    #[test]
    fn render_chunks_handles_empty_input() {
        let ctx = PromptContext {
            project_name: "x",
            project_summary: "",
            chunks: &[],
            scope_hint: "",
            reviewer_feedback: "",
        };
        assert_eq!(
            ctx.render_chunks(),
            "(no relevant code chunks retrieved)".to_string()
        );
    }

    #[test]
    fn render_chunks_truncates_above_cap() {
        let chunks: Vec<_> = (0..(MAX_INLINE_CHUNKS + 5))
            .map(|i| sample_chunk(&format!("f{i}"), ChunkKind::Function, "..."))
            .collect();
        let ctx = PromptContext {
            project_name: "x",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let rendered = ctx.render_chunks();
        // Cap means the last 5 chunks should not appear by name.
        let last_visible = MAX_INLINE_CHUNKS - 1;
        let first_truncated = MAX_INLINE_CHUNKS;
        assert!(rendered.contains(&format!("`f{last_visible}`")));
        assert!(!rendered.contains(&format!("`f{first_truncated}`")));
        assert!(rendered.contains("more chunks omitted"));
    }

    #[test]
    fn module_chunks_label_as_module_with_placeholder_name() {
        let chunks = vec![sample_chunk("", ChunkKind::Module, "import x;\n")];
        let ctx = PromptContext {
            project_name: "x",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let rendered = ctx.render_chunks();
        assert!(rendered.contains("module `<module>`"));
    }

    #[test]
    fn user_text_helper_sets_role_and_content() {
        let m = user_text("hello");
        assert_eq!(m.role, crate::providers::llm::types::Role::User);
        assert_eq!(m.content.len(), 1);
    }

    #[test]
    fn system_text_helper_sets_role_and_content() {
        let m = system_text("be brief");
        assert_eq!(m.role, crate::providers::llm::types::Role::System);
    }

    #[test]
    fn tool_schema_helper_carries_fields() {
        let schema = tool_schema("emit_x", "Emit X.", serde_json::json!({"type": "object"}));
        assert_eq!(schema.name, "emit_x");
        assert_eq!(schema.description, "Emit X.");
        assert_eq!(schema.parameters_schema["type"], "object");
    }
}
