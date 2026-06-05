//! Prompt template: per-function / per-module test cases, version 1.
//!
//! Generates concrete test cases bound to specific code symbols. Each
//! case carries title, preconditions, steps, expected result,
//! priority, and traceability back to the source function/file
//! (rules.md §12.1 — structured output via JSON Schema).

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "test_cases_v1";

pub const TOOL_NAME: &str = "emit_test_cases";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior test engineer writing concrete, executable test cases for \
the supplied scope. Each case must trace back to specific code (file + \
function/method/class).

Rules:
- Bind every test case to a symbol that appears in the supplied chunks. If \
  the symbol is not visible, do NOT generate a case for it.
- Cover positive, negative, boundary, and error paths — but only when each \
  applies to the bound symbol's behavior.
- Steps are imperative and ordered. Expected results are observable, not \
  internal state assertions the test runner cannot reach.
- Priority must follow impact * likelihood, not test difficulty.
- Test case IDs must strictly match the regex `^TC-[A-Z0-9_-]+$` (all-caps, \
  e.g., `TC-LOGIN-SUCCESS`, NOT `TC-Login-Success` or `TC-Login`).
- Always invoke the `emit_test_cases` tool with the structured payload. \
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

    user_body.push_str("## Code to cover\n\n");
    user_body.push_str(&ctx.render_chunks());
    user_body.push_str("\n\nNow invoke `emit_test_cases` with the structured cases.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let priority_enum = serde_json::json!(["p0", "p1", "p2", "p3"]);

    tool_schema(
        TOOL_NAME,
        "Emit a structured set of test cases bound to specific source symbols.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["cases"],
            "properties": {
                "cases": {
                    "type": "array",
                                        "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": [
                            "id",
                            "title",
                            "priority",
                            "steps",
                            "expectedResult"
                        ],
                        "properties": {
                            "id": {
                                "type": "string",
                                "pattern": "^TC-[A-Z0-9_-]+$",
                                "description": "Stable id, prefix `TC-`. MUST use ONLY uppercase letters, digits, hyphens, and underscores (e.g. 'TC-TEST-CARD-FOOTER' in all-caps, NOT 'TC-TEST-CARD-Footer')."
                            },
                            "title": { "type": "string", "minLength": 5, "maxLength": 200 },
                            "priority": { "type": "string", "enum": priority_enum },
                            "preconditions": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 }
                            },
                            "steps": {
                                "type": "array",
                                                                "items": { "type": "string", "minLength": 1 }
                            },
                            "expectedResult": { "type": "string", "minLength": 1 },
                            "traceability": {
                                "type": "array",
                                "items": {
                                    "type": "string",
                                    "minLength": 1,
                                    "description": "Source reference such as `src/routes/auth.ts#login`."
                                }
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
    use crate::providers::llm::types::{Content, Role};
    use crate::services::chunking_service::{Chunk, ChunkKind};

    #[test]
    fn version_matches_filename() {
        assert_eq!(VERSION, "test_cases_v1");
    }

    #[test]
    fn build_messages_emits_system_then_user() {
        let chunks = vec![Chunk {
            kind: ChunkKind::Function,
            name: "add".to_string(),
            start_line: 1,
            end_line: 3,
            content: "function add(a, b) { return a + b; }\n".to_string(),
            token_count: 5,
            oversize: false,
        }];
        let pc = PromptContext {
            project_name: "calc",
            project_summary: "Adder library.",
            chunks: &chunks,
            scope_hint: "src/math.ts",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&pc);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);

        if let Content::Text { text } = &msgs[1].content[0] {
            assert!(text.contains("# Project: calc"));
            assert!(text.contains("Scope: src/math.ts"));
            assert!(text.contains("function `add`"));
        }
    }

    #[test]
    fn tool_id_pattern_is_tc_prefix() {
        let schema = tool();
        let pattern = schema.parameters_schema["properties"]["cases"]["items"]["properties"]["id"]
            ["pattern"]
            .as_str()
            .expect("pattern");
        assert_eq!(pattern, "^TC-[A-Z0-9_-]+$");
    }

    #[test]
    fn priorities_match_shared_schema() {
        let schema = tool();
        let priorities = &schema.parameters_schema["properties"]["cases"]["items"]["properties"]
            ["priority"]["enum"];
        let values: Vec<&str> = priorities
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(values, vec!["p0", "p1", "p2", "p3"]);
    }

    #[test]
    fn traceability_is_optional_string_array() {
        let schema = tool();
        let traceability_items = &schema.parameters_schema["properties"]["cases"]["items"]
            ["properties"]["traceability"]["items"]["type"];
        assert_eq!(traceability_items.as_str(), Some("string"));

        let required = &schema.parameters_schema["properties"]["cases"]["items"]["required"];
        let names: Vec<&str> = required
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(!names.contains(&"traceability"));
    }
}
