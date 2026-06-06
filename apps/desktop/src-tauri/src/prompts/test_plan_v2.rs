//! Prompt template: full-project test plan, version 2.
//!
//! Industry-grade upgrade over `test_plan_v1` (`plan/ARTIFACT_QUALITY_V2.md`
//! Phase 2), an ISO/IEC/IEEE 29119-lite / IEEE 829 backbone: nested
//! `scope { inScope, outOfScope }`, `suspensionCriteria`, explicit
//! `testLevels` / `testTypes` enums, and `deliverables`. Consumed by the
//! artifact-review UI; downstream prompts (`test_cases_v2`) generate the
//! individual cases that satisfy this plan.

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "test_plan_v2";

pub const TOOL_NAME: &str = "emit_test_plan";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior QA architect producing a test plan for the supplied \
project. The plan ships to a human reviewer and must be auditable.

Rules:
- Reference specific files, functions, and modules from the supplied \
  context. Do NOT invent code that does not appear.
- Identify edge cases from the code logic, not generic checklists.
- Prioritize risks by impact * likelihood, not by alphabetical order.
- `scope.inScope` / `scope.outOfScope` name concrete modules, files, or \
  features — never vague areas like 'the backend'.
- `entryCriteria` are preconditions to start testing; `exitCriteria` \
  define done; `suspensionCriteria` state when testing pauses (e.g. \
  blocking defect rate, environment outage) and what resumes it.
- `testLevels` and `testTypes` pick only the values justified by the \
  supplied code; `deliverables` lists the concrete documents/artifacts \
  this plan commits to producing.
- When a section cannot be filled from the supplied context, leave the \
  array empty rather than fabricating items.
- Always invoke the `emit_test_plan` tool with the structured payload. \
  Never reply with free-form prose.

The structured payload MUST have the following JSON structure:
{
  \"summary\": \"One-paragraph overview of the test plan\",
  \"objectives\": [\"Objective 1\", \"Objective 2\"],
  \"scope\": {
    \"inScope\": [\"Module/feature in scope 1\"],
    \"outOfScope\": [\"Module/feature out of scope 1\"]
  },
  \"strategy\": \"Description of the test strategy\",
  \"testLevels\": [\"unit | integration | system | e2e | acceptance\"],
  \"testTypes\": [\"functional | performance | security | usability | reliability | compatibility | regression\"],
  \"environments\": [\"Environment 1\"],
  \"risks\": [
    {
      \"description\": \"Risk description\",
      \"mitigation\": \"Mitigation strategy\"
    }
  ],
  \"entryCriteria\": [\"Entry criteria 1\"],
  \"exitCriteria\": [\"Exit criteria 1\"],
  \"suspensionCriteria\": [\"Suspension criteria 1\"],
  \"deliverables\": [\"Deliverable 1\"]
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
      \"scope\": {\n\
        \"inScope\": [\"Module/feature in scope\"],\n\
        \"outOfScope\": [\"Module/feature out of scope\"]\n\
      },\n\
      \"strategy\": \"Description of the test strategy\",\n\
      \"testLevels\": [\"unit | integration | system | e2e | acceptance\"],\n\
      \"testTypes\": [\"functional | performance | security | usability | reliability | compatibility | regression\"],\n\
      \"environments\": [\"Environment 1\"],\n\
      \"risks\": [\n\
        {\n\
          \"description\": \"Risk description\",\n\
          \"mitigation\": \"Mitigation strategy\"\n\
        }\n\
      ],\n\
      \"entryCriteria\": [\"Entry criteria 1\"],\n\
      \"exitCriteria\": [\"Exit criteria 1\"],\n\
      \"suspensionCriteria\": [\"Suspension criteria 1\"],\n\
      \"deliverables\": [\"Deliverable 1\"]\n\
    }\n\
    `scope` is a nested OBJECT with `inScope` and `outOfScope` arrays — not flat top-level keys. `testLevels` and `testTypes` only accept the enum values listed above.\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let test_level_enum = serde_json::json!(["unit", "integration", "system", "e2e", "acceptance"]);
    let test_type_enum = serde_json::json!([
        "functional",
        "performance",
        "security",
        "usability",
        "reliability",
        "compatibility",
        "regression"
    ]);

    tool_schema(
        TOOL_NAME,
        "Emit a structured test plan for the supplied project.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "summary",
                "objectives",
                "scope",
                "strategy",
                "testLevels",
                "testTypes",
                "environments",
                "risks",
                "entryCriteria",
                "exitCriteria",
                "suspensionCriteria",
                "deliverables"
            ],
            "properties": {
                "summary": { "type": "string", "minLength": 1, "maxLength": 1500 },
                "objectives": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "scope": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["inScope", "outOfScope"],
                    "properties": {
                        "inScope": {
                            "type": "array",
                            "items": { "type": "string", "minLength": 1 }
                        },
                        "outOfScope": {
                            "type": "array",
                            "items": { "type": "string", "minLength": 1 }
                        }
                    }
                },
                "strategy": { "type": "string", "minLength": 1, "maxLength": 2000 },
                "testLevels": {
                    "type": "array",
                    "items": { "type": "string", "enum": test_level_enum }
                },
                "testTypes": {
                    "type": "array",
                    "items": { "type": "string", "enum": test_type_enum }
                },
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
                },
                "suspensionCriteria": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 }
                },
                "deliverables": {
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
        assert_eq!(VERSION, "test_plan_v2");
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
            panic!("expected text content");
        }
    }

    #[test]
    fn scope_is_nested_in_out_object() {
        let schema = tool();
        let scope = &schema.parameters_schema["properties"]["scope"];
        assert_eq!(scope["type"].as_str(), Some("object"));
        let required: Vec<&str> = scope["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(required, vec!["inScope", "outOfScope"]);

        // Flat v1-style scopeIn/scopeOut keys are gone.
        let props = schema.parameters_schema["properties"]
            .as_object()
            .expect("object");
        assert!(!props.contains_key("scopeIn"));
        assert!(!props.contains_key("scopeOut"));
    }

    #[test]
    fn tool_requires_29119_backbone_sections() {
        let schema = tool();
        let required: Vec<&str> = schema.parameters_schema["required"]
            .as_array()
            .expect("required")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for key in [
            "scope",
            "entryCriteria",
            "exitCriteria",
            "suspensionCriteria",
            "testLevels",
            "testTypes",
            "deliverables",
        ] {
            assert!(required.contains(&key), "missing required `{key}`");
        }
    }

    #[test]
    fn test_levels_and_types_are_enumerated() {
        let schema = tool();
        let levels: Vec<&str> = schema.parameters_schema["properties"]["testLevels"]["items"]
            ["enum"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            levels,
            vec!["unit", "integration", "system", "e2e", "acceptance"]
        );

        let types: Vec<&str> = schema.parameters_schema["properties"]["testTypes"]["items"]
            ["enum"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(types.contains(&"functional"));
        assert!(types.contains(&"security"));
        assert!(types.contains(&"regression"));
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
