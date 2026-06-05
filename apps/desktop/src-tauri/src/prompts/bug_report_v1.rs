//! Prompt template: bug report (runtime issue), version 1.
//!
//! Different from a defect report: defects are static-analysis
//! findings, bugs are concrete failures the user / automated tests
//! observed at runtime. The model formats them for issue-tracker
//! import (Steps to Reproduce, Expected vs Actual, Severity, Root
//! Cause Analysis).

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "bug_report_v1";

pub const TOOL_NAME: &str = "emit_bug_report";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior engineer formatting a runtime-issue report for an issue \
tracker. The output must be self-contained — a triage engineer should \
understand the problem without reading the surrounding context.

Rules:
- Steps to reproduce are imperative, ordered, and minimal — strip \
  irrelevant setup.
- Expected vs Actual must be concrete and observable.
- Root cause analysis cites specific symbols / line ranges from the \
  supplied chunks. Do NOT speculate beyond what the code shows.
- One bug per report. If multiple defects share a symptom, emit \
  multiple reports rather than coalescing.
- Bug IDs must strictly match the regex `^BUG-[A-Z0-9_-]+$` (all-caps, \
  e.g., `BUG-SESSION-LEAK`, NOT `BUG-Session-Leak` or `BUG-Session`).
- Always invoke the `emit_bug_report` tool with the structured payload. \
  Never reply with free-form prose.";

#[must_use]
pub fn build_messages(ctx: &PromptContext<'_>) -> Vec<Message> {
    let mut user_body = String::new();
    writeln!(user_body, "# Project: {}\n", ctx.project_name).expect("write");

    if !ctx.scope_hint.is_empty() {
        writeln!(user_body, "Scope: {}\n", ctx.scope_hint).expect("write");
    }

    if !ctx.project_summary.is_empty() {
        user_body.push_str("## Project context\n\n");
        user_body.push_str(ctx.project_summary);
        user_body.push_str("\n\n");
    }

    if !ctx.reviewer_feedback.is_empty() {
        user_body.push_str("## Reviewer feedback / observed symptoms\n\n");
        user_body.push_str(ctx.reviewer_feedback);
        user_body.push_str("\n\n");
    }

    user_body.push_str("## Code in scope\n\n");
    user_body.push_str(&ctx.render_chunks());
    user_body.push_str("\n\nNow invoke `emit_bug_report` with the structured report.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let severity_enum = serde_json::json!(["critical", "major", "minor", "trivial"]);

    tool_schema(
        TOOL_NAME,
        "Emit a structured bug report ready for issue-tracker import.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["bugs"],
            "properties": {
                "bugs": {
                    "type": "array",
                                        "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": [
                            "id",
                            "title",
                            "severity",
                            "steps_to_reproduce",
                            "expected_behavior",
                            "actual_behavior",
                            "root_cause"
                        ],
                        "properties": {
                            "id": {
                                "type": "string",
                                "pattern": "^BUG-[A-Z0-9_-]+$",
                                "description": "Stable id, prefix `BUG-`. MUST use ONLY uppercase letters, digits, hyphens, and underscores (e.g. 'BUG-SESSION-LEAK' in all-caps, NOT 'BUG-Session-Leak')."
                            },
                            "title": { "type": "string", "minLength": 10, "maxLength": 200 },
                            "severity": { "type": "string", "enum": severity_enum },
                            "environment": {
                                "type": "string",
                                "description": "OS / runtime / version stack when known."
                            },
                            "steps_to_reproduce": {
                                "type": "array",
                                                                "items": { "type": "string", "minLength": 1 }
                            },
                            "expected_behavior": { "type": "string", "minLength": 1 },
                            "actual_behavior": { "type": "string", "minLength": 1 },
                            "root_cause": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["symbol", "explanation"],
                                "properties": {
                                    "symbol": { "type": "string", "minLength": 1 },
                                    "start_line": { "type": "integer", "minimum": 1 },
                                    "end_line": { "type": "integer", "minimum": 1 },
                                    "file_hint": { "type": "string" },
                                    "explanation": { "type": "string", "minLength": 10 }
                                }
                            },
                            "evidence_snippet": {
                                "type": "string",
                                "description": "Short verbatim quote of the offending code."
                            }
                        }
                    }
                }
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::chunking_service::{Chunk, ChunkKind};

    fn fixture() -> Vec<Chunk> {
        vec![Chunk {
            kind: ChunkKind::Function,
            name: "save".to_string(),
            start_line: 5,
            end_line: 15,
            content: "function save() { /* race */ }\n".to_string(),
            token_count: 5,
            oversize: false,
        }]
    }

    #[test]
    fn version_matches_filename() {
        assert_eq!(VERSION, "bug_report_v1");
    }

    #[test]
    fn build_messages_emits_two_messages() {
        let chunks = fixture();
        let pc = PromptContext {
            project_name: "demo",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "Observed double-write under load.",
        };
        let msgs = build_messages(&pc);
        assert_eq!(msgs.len(), 2);
        if let crate::providers::llm::types::Content::Text { text } = &msgs[1].content[0] {
            assert!(text.contains("Observed double-write"));
        }
    }

    #[test]
    fn id_pattern_is_bug_prefix() {
        let schema = tool();
        let pattern = schema.parameters_schema["properties"]["bugs"]["items"]["properties"]["id"]
            ["pattern"]
            .as_str()
            .expect("pattern");
        assert_eq!(pattern, "^BUG-[A-Z0-9_-]+$");
    }

    #[test]
    fn root_cause_requires_symbol_and_explanation() {
        let schema = tool();
        let req = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"]
            ["root_cause"]["required"];
        let v: Vec<&str> = req
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(v.contains(&"symbol"));
        assert!(v.contains(&"explanation"));
    }

    #[test]
    fn severity_enum_is_four_levels() {
        let schema = tool();
        let sev = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"]
            ["severity"]["enum"];
        let v: Vec<&str> = sev
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(v, vec!["critical", "major", "minor", "trivial"]);
    }
}
