//! Prompt template: per-function / per-module test cases, version 2.
//!
//! Industry-grade upgrade over `test_cases_v1` (`plan/ARTIFACT_QUALITY_V2.md`
//! Phase 1): `TestRail` separated-steps pattern (`steps` is now
//! `{ action, expectedResult }[]`), explicit case `type`
//! (positive/negative/boundary/error/security), optional `testData`,
//! `postconditions[]`, and a prompt mandate of at least one negative and
//! one boundary case per covered feature. The runnable `files[]` payload
//! is byte-identical to v1 — the sandbox runner contract is unchanged.

use crate::providers::llm::types::{Message, ToolSchema};

use super::{
    render_user_body, runnable_files_schema, system_text, tool_schema, user_text, PromptContext,
    UserBodyOptions,
};

pub const VERSION: &str = "test_cases_v2";

pub const TOOL_NAME: &str = "emit_test_cases";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior test engineer writing concrete, executable test cases for \
the supplied scope. Each case must trace back to specific code (file + \
function/method/class).

Rules:
- Bind every test case to a symbol that appears in the supplied chunks. If \
  the symbol is not visible, do NOT generate a case for it.
- Every case carries a `type`: positive, negative, boundary, error, or \
  security. For EACH covered feature you MUST include at least one \
  `negative` case (invalid input / unexpected usage) and at least one \
  `boundary` case (boundary-value analysis: min, max, empty, off-by-one).
- Steps follow the separated-steps pattern: each step is an object with an \
  imperative `action` and an observable `expectedResult` for that step. \
  Expected results are observable, not internal state assertions the test \
  runner cannot reach.
- Supply `testData` (concrete input values) when the case depends on \
  specific data; supply `postconditions` when the case leaves state that \
  must be verified or cleaned up.
- Priority must follow impact * likelihood, not test difficulty.
- Test case IDs must strictly match the regex `^TC-[A-Z0-9_-]+$` (all-caps, \
  e.g., `TC-LOGIN-SUCCESS`, NOT `TC-Login-Success` or `TC-Login`).
- Also emit a `files` array that makes the cases runnable in the local \
  sandbox: the minimal source-under-test file(s), reproduced from the \
  supplied chunks and marked `isTest: false`, plus one vitest spec per \
  source file marked `isTest: true`. Specs use \
  `import { describe, it, expect } from 'vitest'` and import the source by \
  relative path. Use workspace-relative paths only — never an absolute \
  path or a `..` segment. Omit `files` only when the scope has no \
  executable behavior (e.g. pure type declarations).
- Always invoke the `emit_test_cases` tool with the structured payload. \
  Never reply with free-form prose.

The structured payload MUST have the following JSON structure:
{
  \"cases\": [
    {
      \"id\": \"TC-UNIQUE-ID\",
      \"title\": \"Short descriptive title\",
      \"type\": \"positive | negative | boundary | error | security\",
      \"priority\": \"p0 | p1 | p2 | p3\",
      \"preconditions\": [\"Precondition 1\"],
      \"testData\": \"Concrete input values used by the steps\",
      \"steps\": [
        { \"action\": \"Step 1 action\", \"expectedResult\": \"Observable result of step 1\" }
      ],
      \"postconditions\": [\"State left after the case\"],
      \"traceability\": [\"path/to/file.ext#symbol\"]
    }
  ]
}";

#[must_use]
pub fn build_messages(ctx: &PromptContext<'_>) -> Vec<Message> {
    let mut user_body = render_user_body(
        ctx,
        &UserBodyOptions {
            context_heading: "Project context",
            empty_context_note: None,
            feedback_heading: "Reviewer feedback",
            code_heading: "Code to cover",
        },
    );
    user_body.push_str("\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_test_cases` tool with the structured cases.\n\
    The JSON payload MUST use ONLY these top-level keys: `cases` (required) and `files` (optional — the runnable workspace):\n\
    {\n\
      \"cases\": [\n\
        {\n\
          \"id\": \"TC-UNIQUE-ID\",\n\
          \"title\": \"Short descriptive title\",\n\
          \"type\": \"positive | negative | boundary | error | security\",\n\
          \"priority\": \"p0 | p1 | p2 | p3\",\n\
          \"preconditions\": [\"Precondition 1\"],\n\
          \"testData\": \"Concrete input values used by the steps\",\n\
          \"steps\": [\n\
            { \"action\": \"Step 1 action\", \"expectedResult\": \"Observable result of step 1\" }\n\
          ],\n\
          \"postconditions\": [\"State left after the case\"],\n\
          \"traceability\": [\"path/to/file.ext#symbol\"]\n\
        }\n\
      ],\n\
      \"files\": [\n\
        { \"path\": \"src/add.ts\", \"contents\": \"...\", \"isTest\": false },\n\
        { \"path\": \"add.test.ts\", \"contents\": \"...\", \"isTest\": true }\n\
      ]\n\
    }\n\
    Every `steps` entry is an OBJECT with `action` and `expectedResult` — never a plain string. Include at least one `negative` and one `boundary` case per covered feature.\n\
    Include `files` so the cases run in the local sandbox: the minimal source-under-test (marked isTest:false) plus one vitest spec per source file (marked isTest:true), using workspace-relative paths only (no absolute paths, no `..`). Omit `files` only when the scope has no executable behavior.\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let priority_enum = serde_json::json!(["p0", "p1", "p2", "p3"]);
    let type_enum = serde_json::json!(["positive", "negative", "boundary", "error", "security"]);

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
                            "type",
                            "priority",
                            "steps"
                        ],
                        "properties": {
                            "id": {
                                "type": "string",
                                "pattern": "^TC-[A-Z0-9_-]+$",
                                "description": "Stable id, prefix `TC-`. MUST use ONLY uppercase letters, digits, hyphens, and underscores (e.g. 'TC-TEST-CARD-FOOTER' in all-caps, NOT 'TC-TEST-CARD-Footer')."
                            },
                            "title": { "type": "string", "minLength": 5, "maxLength": 200 },
                            "type": {
                                "type": "string",
                                "enum": type_enum,
                                "description": "Test design category. Each covered feature needs at least one `negative` and one `boundary` case."
                            },
                            "priority": { "type": "string", "enum": priority_enum },
                            "preconditions": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 }
                            },
                            "testData": {
                                "type": "string",
                                "description": "Concrete input values / fixtures the steps rely on."
                            },
                            "steps": {
                                "type": "array",
                                "minItems": 1,
                                "items": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["action", "expectedResult"],
                                    "properties": {
                                        "action": { "type": "string", "minLength": 1 },
                                        "expectedResult": { "type": "string", "minLength": 1 }
                                    }
                                }
                            },
                            "postconditions": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 }
                            },
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
                },
                "files": runnable_files_schema()
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
        assert_eq!(VERSION, "test_cases_v2");
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
    fn case_type_enum_matches_shared_schema() {
        let schema = tool();
        let types = &schema.parameters_schema["properties"]["cases"]["items"]["properties"]
            ["type"]["enum"];
        let values: Vec<&str> = types
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            values,
            vec!["positive", "negative", "boundary", "error", "security"]
        );
    }

    #[test]
    fn steps_are_separated_action_expected_result_objects() {
        let schema = tool();
        let steps = &schema.parameters_schema["properties"]["cases"]["items"]["properties"]
            ["steps"];
        assert_eq!(steps["minItems"].as_u64(), Some(1));
        let required: Vec<&str> = steps["items"]["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(required, vec!["action", "expectedResult"]);
    }

    #[test]
    fn case_requires_type_but_not_legacy_expected_result() {
        let schema = tool();
        let required: Vec<&str> = schema.parameters_schema["properties"]["cases"]["items"]
            ["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"type"));
        assert!(required.contains(&"steps"));
        // v1's single top-level expectedResult is replaced by per-step results.
        assert!(!required.contains(&"expectedResult"));
        let props = &schema.parameters_schema["properties"]["cases"]["items"]["properties"];
        assert!(props.get("expectedResult").is_none());
    }

    #[test]
    fn prompt_mandates_negative_and_boundary_cases() {
        assert!(SYSTEM_INSTRUCTIONS.contains("at least one `negative` case"));
        assert!(SYSTEM_INSTRUCTIONS.contains("at least one `boundary` case"));
    }

    #[test]
    fn files_array_contract_is_byte_identical_to_v1() {
        // The sandbox runner contract must not drift: the v2 `files`
        // schema is the same JSON value as v1's.
        let v1 = super::super::test_cases_v1::tool();
        let v2 = tool();
        assert_eq!(
            v1.parameters_schema["properties"]["files"],
            v2.parameters_schema["properties"]["files"]
        );
    }
}
