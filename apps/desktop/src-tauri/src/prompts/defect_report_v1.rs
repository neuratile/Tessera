//! Prompt template: static-analysis defect report, version 1.
//!
//! Asks the model to identify potential defects from the supplied
//! chunks: logic errors, race conditions, null safety, input
//! validation, security, performance, memory leaks, error handling.
//! Each finding carries a confidence score so the human reviewer can
//! triage by signal strength.

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "defect_report_v1";

pub const TOOL_NAME: &str = "emit_defect_report";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior code reviewer running a static-analysis pass. The output \
ships to a human triage queue; precision matters more than recall.

Rules:
- Only report findings with HIGH or MEDIUM confidence. Do NOT pad with \
  speculative low-confidence noise.
- Cite a specific symbol + line range from the supplied chunks for every \
  finding. If the location is not visible in the chunks, do NOT report it.
- Suggested fixes must be concrete code changes, not 'consider refactoring'.
- Categorize accurately — over-broad categories make triage harder.
- Defect IDs must strictly match the regex `^DEF-[A-Z0-9_-]+$` (all-caps, \
  e.g., `DEF-NULL-POINTER`, NOT `DEF-Null-Pointer` or `DEF-Null`).
- Always invoke the `emit_defect_report` tool with the structured payload. \
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
        user_body.push_str("## Reviewer feedback\n\n");
        user_body.push_str(ctx.reviewer_feedback);
        user_body.push_str("\n\n");
    }

    user_body.push_str("## Code under review\n\n");
    user_body.push_str(&ctx.render_chunks());
    user_body.push_str("\n\nNow invoke `emit_defect_report` with the structured findings.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let severity_enum = serde_json::json!(["critical", "major", "minor", "trivial"]);
    let confidence_enum = serde_json::json!(["high", "medium"]);
    let category_enum = serde_json::json!([
        "logic_error",
        "race_condition",
        "null_safety",
        "input_validation",
        "security",
        "performance",
        "memory_leak",
        "error_handling"
    ]);

    tool_schema(
        TOOL_NAME,
        "Emit a structured defect report of static-analysis findings.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["findings"],
            "properties": {
                "findings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": [
                            "id",
                            "severity",
                            "category",
                            "confidence",
                            "location",
                            "description",
                            "impact",
                            "suggested_fix"
                        ],
                        "properties": {
                            "id": {
                                "type": "string",
                                "pattern": "^DEF-[A-Z0-9_-]+$",
                                "description": "Stable id, prefix `DEF-`. MUST use ONLY uppercase letters, digits, hyphens, and underscores (e.g. 'DEF-NULL-POINTER' in all-caps, NOT 'DEF-Null-Pointer')."
                            },
                            "severity": { "type": "string", "enum": severity_enum },
                            "category": { "type": "string", "enum": category_enum },
                            "confidence": { "type": "string", "enum": confidence_enum },
                            "location": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["symbol", "start_line", "end_line"],
                                "properties": {
                                    "symbol": { "type": "string", "minLength": 1 },
                                    "start_line": { "type": "integer", "minimum": 1 },
                                    "end_line": { "type": "integer", "minimum": 1 },
                                    "file_hint": { "type": "string" }
                                }
                            },
                            "description": { "type": "string", "minLength": 10 },
                            "impact": { "type": "string", "minLength": 5 },
                            "suggested_fix": { "type": "string", "minLength": 5 }
                        }
                    }
                },
                "summary": {
                    "type": "string",
                    "description": "One-paragraph overview written after the findings list."
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
            name: "parseUser".to_string(),
            start_line: 1,
            end_line: 10,
            content: "function parseUser(s) { return JSON.parse(s); }\n".to_string(),
            token_count: 10,
            oversize: false,
        }]
    }

    #[test]
    fn version_matches_filename() {
        assert_eq!(VERSION, "defect_report_v1");
    }

    #[test]
    fn build_messages_emits_system_and_user() {
        let chunks = fixture();
        let pc = PromptContext {
            project_name: "demo",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&pc);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn confidence_enum_is_high_or_medium_only() {
        // rules.md §12.4: never silently retry / hallucinate. The
        // confidence enum deliberately excludes "low" — low-confidence
        // findings are wasted human-review time.
        let schema = tool();
        let conf = &schema.parameters_schema["properties"]["findings"]["items"]["properties"]
            ["confidence"]["enum"];
        let v: Vec<&str> = conf
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(v, vec!["high", "medium"]);
    }

    #[test]
    fn category_enum_covers_eight_buckets() {
        let schema = tool();
        let cats = &schema.parameters_schema["properties"]["findings"]["items"]["properties"]
            ["category"]["enum"];
        let v: Vec<&str> = cats
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(v.len(), 8);
        assert!(v.contains(&"security"));
        assert!(v.contains(&"null_safety"));
    }

    #[test]
    fn id_pattern_is_def_prefix() {
        let schema = tool();
        let pattern = schema.parameters_schema["properties"]["findings"]["items"]["properties"]
            ["id"]["pattern"]
            .as_str()
            .expect("pattern");
        assert_eq!(pattern, "^DEF-[A-Z0-9_-]+$");
    }

    #[test]
    fn location_requires_symbol_and_line_range() {
        let schema = tool();
        let req = &schema.parameters_schema["properties"]["findings"]["items"]["properties"]
            ["location"]["required"];
        let v: Vec<&str> = req
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(v.contains(&"symbol"));
        assert!(v.contains(&"start_line"));
        assert!(v.contains(&"end_line"));
    }
}
