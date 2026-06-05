//! Prompt template: full-project test plan, version 1.
//!
//! Produces the high-level test strategy document — scope,
//! objectives, strategy, environments, risk matrix, entry / exit
//! criteria. Consumed by the artifact-review UI; downstream prompts
//! (`test_cases_v1`) generate the individual cases that satisfy this
//! plan.

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "test_plan_v1";

pub const TOOL_NAME: &str = "emit_test_plan";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior QA architect producing a test plan for the supplied \
project. The plan ships to a human reviewer and must be auditable.

Rules:
- Reference specific files, functions, and modules from the supplied \
  context. Do NOT invent code that does not appear.
- Identify edge cases from the code logic, not generic checklists.
- Prioritize risks by impact * likelihood, not by alphabetical order.
- When a section cannot be filled from the supplied context, leave the \
  array empty rather than fabricating items.
- Always invoke the `emit_test_plan` tool with the structured payload. \
  Never reply with free-form prose.

The structured payload MUST have the following JSON structure:
{
  \"summary\": \"One-paragraph overview of the test plan\",
  \"objectives\": [\"Objective 1\", \"Objective 2\"],
  \"scopeIn\": [\"Module/feature in scope 1\"],
  \"scopeOut\": [\"Module/feature out of scope 1\"],
  \"strategy\": \"Description of the test strategy\",
  \"environments\": [\"Environment 1\"],
  \"risks\": [
    {
      \"description\": \"Risk description\",
      \"mitigation\": \"Mitigation strategy\"
    }
  ],
  \"entryCriteria\": [\"Entry criteria 1\"],
  \"exitCriteria\": [\"Exit criteria 1\"]
}";

#[must_use]
pub fn build_messages(ctx: &PromptContext<'_>) -> Vec<Message> {
    let mut user_body = String::new();
    writeln!(user_body, "# Project: {}\n", ctx.project_name).expect("write");

    if !ctx.scope_hint.is_empty() {
        writeln!(user_body, "Scope: {}\n", ctx.scope_hint).expect("write");
    }

    user_body.push_str("## Project context (auto-generated)\n\n");
    if ctx.project_summary.is_empty() {
        user_body.push_str("(none — proceed from chunks alone)\n\n");
    } else {
        user_body.push_str(ctx.project_summary);
        user_body.push_str("\n\n");
    }

    if !ctx.reviewer_feedback.is_empty() {
        user_body.push_str("## Previous reviewer feedback\n\n");
        user_body.push_str(ctx.reviewer_feedback);
        user_body.push_str("\n\n");
    }

    user_body.push_str("## Relevant code\n\n");
    user_body.push_str(&ctx.render_chunks());
    user_body.push_str("\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_test_plan` tool with the structured plan.\n\
    The JSON payload MUST have exactly these keys (and no others):\n\
    {\n\
      \"summary\": \"One-paragraph overview of the test plan\",\n\
      \"objectives\": [\"Objective 1\"],\n\
      \"scopeIn\": [\"Module/feature in scope\"],\n\
      \"scopeOut\": [\"Module/feature out of scope\"],\n\
      \"strategy\": \"Description of the test strategy\",\n\
      \"environments\": [\"Environment 1\"],\n\
      \"risks\": [\n\
        {\n\
          \"description\": \"Risk description\",\n\
          \"mitigation\": \"Mitigation strategy\"\n\
        }\n\
      ],\n\
      \"entryCriteria\": [\"Entry criteria 1\"],\n\
      \"exitCriteria\": [\"Exit criteria 1\"]\n\
    }\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    tool_schema(
        TOOL_NAME,
        "Emit a structured test plan for the supplied project.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "summary",
                "objectives",
                "scopeIn",
                "scopeOut",
                "strategy",
                "environments",
                "risks",
                "entryCriteria",
                "exitCriteria"
            ],
            "properties": {
                "summary": { "type": "string", "minLength": 1, "maxLength": 1500 },
                "objectives": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "scopeIn": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "scopeOut": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "strategy": { "type": "string", "minLength": 1, "maxLength": 2000 },
                "environments": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "risks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["description"],
                        "properties": {
                            "description": { "type": "string", "minLength": 1 },
                            "mitigation": { "type": "string", "minLength": 1 }
                        }
                    }
                },
                "entryCriteria": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "exitCriteria": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
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

    fn ctx() -> Vec<Chunk> {
        vec![Chunk {
            kind: ChunkKind::Function,
            name: "login".to_string(),
            start_line: 10,
            end_line: 30,
            content: "function login(creds) {}\n".to_string(),
            token_count: 5,
            oversize: false,
        }]
    }

    #[test]
    fn version_matches_filename() {
        assert_eq!(VERSION, "test_plan_v1");
    }

    #[test]
    fn build_messages_emits_two_messages() {
        let chunks = ctx();
        let pc = PromptContext {
            project_name: "myapp",
            project_summary: "Auth service.",
            chunks: &chunks,
            scope_hint: "auth module",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&pc);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn user_body_mentions_project_summary() {
        let chunks = ctx();
        let pc = PromptContext {
            project_name: "myapp",
            project_summary: "Hand-rolled auth in Express.",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&pc);
        if let Content::Text { text } = &msgs[1].content[0] {
            assert!(text.contains("Hand-rolled auth in Express."));
            assert!(text.contains("function `login`"));
            assert!(text.contains("`emit_test_plan`"));
        } else {
            panic!("expected text content");
        }
    }

    #[test]
    fn empty_summary_falls_back_to_chunks_only_note() {
        let chunks = ctx();
        let pc = PromptContext {
            project_name: "myapp",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&pc);
        if let Content::Text { text } = &msgs[1].content[0] {
            assert!(text.contains("(none — proceed from chunks alone)"));
        } else {
            panic!();
        }
    }

    #[test]
    fn tool_lists_required_top_level_fields() {
        let schema = tool();
        let required = schema.parameters_schema["required"]
            .as_array()
            .expect("required");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"summary"));
        assert!(names.contains(&"objectives"));
        assert!(names.contains(&"scopeIn"));
        assert!(names.contains(&"strategy"));
        assert!(names.contains(&"entryCriteria"));
        assert!(names.contains(&"exitCriteria"));
    }

    #[test]
    fn risks_only_require_description() {
        let schema = tool();
        let required = &schema.parameters_schema["properties"]["risks"]["items"]["required"];
        let names: Vec<&str> = required
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(names, vec!["description"]);
    }
}
