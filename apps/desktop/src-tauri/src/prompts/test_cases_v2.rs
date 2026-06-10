//! Prompt template: per-function / per-module test cases, version 2.
//!
//! Industry-grade upgrade over `test_cases_v1` (`plan/ARTIFACT_QUALITY_V2.md`
//! Phase 1): `TestRail` separated-steps pattern (`steps` is now
//! `{ action, expectedResult }[]`), explicit case `type`
//! (positive/negative/boundary/error/security), optional `testData`,
//! `postconditions[]`, and a prompt mandate of at least one negative and
//! one boundary case per covered feature. The runnable `files[]` payload
//! is byte-identical to v1 — the sandbox runner contract is unchanged.
//!
//! The runnable-files instruction is language-conditional
//! (`plan/SANDBOX_PYTHON_RUNNER.md` §7): JS/TS scopes get the original
//! vitest wording (byte-identical, locked by the existing snapshots);
//! Python scopes get a pytest variant whose test-function naming
//! convention (`test_<tc_id_snake>__<description>`) the `docker_py`
//! runner re-hyphenates back to `TC-…` ids. The tool schema — and with
//! it the `files[]` contract — is shared and unchanged, so `VERSION`
//! stays `test_cases_v2`.

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
- Each generated spec's top-level `it`/`test` title MUST begin with the \
  owning case `id` as its first token, e.g. \
  `it('TC-LOGIN-01 rejects empty password', …)`. The sandbox runner parses \
  that leading token to map an assertion's pass/fail back to its test case.
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

/// Python variant of [`SYSTEM_INSTRUCTIONS`]
/// (`plan/SANDBOX_PYTHON_RUNNER.md` §7): identical case rules, but the
/// runnable `files[]` are pytest files (stdlib + pytest only — the sandbox
/// has no third-party packages and no network) and the case-id bridge
/// lives in the test *function name*, since Python identifiers cannot
/// carry the JS `'TC-LOGIN-01 …'` title convention.
const SYSTEM_INSTRUCTIONS_PY: &str = "\
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
  supplied chunks and marked `isTest: false`, plus one pytest file per \
  source module, named `test_<module>.py` and marked `isTest: true`. Tests \
  import the source by its workspace-relative module path and may use ONLY \
  the Python standard library plus pytest — no third-party packages are \
  installed in the sandbox and it has no network. Use workspace-relative \
  paths only — never an absolute path or a `..` segment. Omit `files` only \
  when the scope has no executable behavior (e.g. pure type stubs).
- Each generated test function name MUST begin with the owning case `id`, \
  lower-snake-cased directly after the `test_` prefix and separated from \
  the description by a double underscore, e.g. \
  `def test_tc_login_01__rejects_empty_password():` for case \
  `TC-LOGIN-01`. The sandbox runner uppercases and re-hyphenates that \
  leading token to map an assertion's pass/fail back to its test case.
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

/// Whether the generation scope is Python (`plan/SANDBOX_PYTHON_RUNNER.md`
/// §7). The chunks carry no file paths, so detection uses the caller's
/// `scope_hint` extension when one is present, falling back to a
/// deterministic syntax vote over the chunk contents. Ties (no signal
/// either way) default to the JS/TS instruction — the pre-Python behavior.
fn is_python_scope(ctx: &PromptContext<'_>) -> bool {
    // The hint is lowercased up front, so the suffix comparisons below are
    // effectively case-insensitive.
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    {
        let hint = ctx.scope_hint.trim().to_ascii_lowercase();
        if hint.ends_with(".py") {
            return true;
        }
        if [".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|ext| hint.ends_with(ext))
        {
            return false;
        }
    }

    let mut python = 0usize;
    let mut js = 0usize;
    for chunk in ctx.chunks {
        for line in chunk.content.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let block_header = (t.starts_with("def ")
                || t.starts_with("async def ")
                || t.starts_with("class ")
                || t.starts_with("elif ")
                || t.starts_with("except"))
                && t.ends_with(':');
            if block_header || t == "pass" {
                python += 1;
            }
            if t.ends_with(';')
                || t.ends_with('{')
                || t.contains("=>")
                || t.starts_with("function ")
                || t.starts_with("const ")
                || t.starts_with("let ")
                || t.starts_with("export ")
                || t.starts_with("interface ")
            {
                js += 1;
            }
        }
    }
    python > js
}

/// `[CRITICAL INSTRUCTION]` tail appended to the user body for JS/TS
/// scopes. Byte-identical to the pre-Python wording — locked by the
/// `test_cases_v2_messages` snapshot.
const USER_TAIL: &str = "\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_test_cases` tool with the structured cases.\n\
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
    Each spec's top-level `it`/`test` title MUST begin with the owning case `id` token (e.g. `it('TC-LOGIN-01 rejects empty password', …)`) so sandbox results map back to the case.\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.";

/// Python variant of [`USER_TAIL`] — pytest files + the
/// `test_<tc_id_snake>__<description>` naming bridge.
const USER_TAIL_PY: &str = "\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_test_cases` tool with the structured cases.\n\
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
        { \"path\": \"src/add.py\", \"contents\": \"...\", \"isTest\": false },\n\
        { \"path\": \"test_add.py\", \"contents\": \"...\", \"isTest\": true }\n\
      ]\n\
    }\n\
    Every `steps` entry is an OBJECT with `action` and `expectedResult` — never a plain string. Include at least one `negative` and one `boundary` case per covered feature.\n\
    Include `files` so the cases run in the local sandbox: the minimal source-under-test (marked isTest:false) plus one pytest file per source module, named `test_<module>.py` (marked isTest:true), using workspace-relative paths only (no absolute paths, no `..`). Tests may use ONLY the Python standard library plus pytest — no third-party packages exist in the sandbox. Omit `files` only when the scope has no executable behavior.\n\
    Each test function name MUST begin with the owning case `id` lower-snake-cased after the `test_` prefix, separated from the description by a double underscore (e.g. `def test_tc_login_01__rejects_empty_password():` for `TC-LOGIN-01`) so sandbox results map back to the case.\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.";

#[must_use]
pub fn build_messages(ctx: &PromptContext<'_>) -> Vec<Message> {
    let python = is_python_scope(ctx);
    let mut user_body = render_user_body(
        ctx,
        &UserBodyOptions {
            context_heading: "Project context",
            empty_context_note: None,
            feedback_heading: "Reviewer feedback",
            code_heading: "Code to cover",
        },
    );
    user_body.push_str(if python { USER_TAIL_PY } else { USER_TAIL });

    let system = if python {
        SYSTEM_INSTRUCTIONS_PY
    } else {
        SYSTEM_INSTRUCTIONS
    };
    vec![system_text(system), user_text(user_body)]
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
    fn prompt_requires_tc_id_prefixed_spec_titles() {
        // plan/TEST_CASE_TABLE.md §4.2 name→id bridge: specs must lead
        // with the case id so sandbox results re-attach to the case.
        assert!(SYSTEM_INSTRUCTIONS.contains("MUST begin with the"));
        assert!(SYSTEM_INSTRUCTIONS.contains("TC-LOGIN-01 rejects empty password"));
    }

    fn py_chunk() -> Chunk {
        Chunk {
            kind: ChunkKind::Function,
            name: "add".to_string(),
            start_line: 1,
            end_line: 2,
            content: "def add(a, b):\n    return a + b\n".to_string(),
            token_count: 8,
            oversize: false,
        }
    }

    #[test]
    fn is_python_scope_prefers_the_scope_hint_extension() {
        let chunks = vec![py_chunk()];
        // .py hint → Python, even before looking at the chunks.
        let py = PromptContext {
            project_name: "calc",
            project_summary: "",
            chunks: &[],
            scope_hint: "src/add.py",
            reviewer_feedback: "",
        };
        assert!(is_python_scope(&py));
        // JS-family hint wins over Python-looking content.
        let ts = PromptContext {
            project_name: "calc",
            project_summary: "",
            chunks: &chunks,
            scope_hint: "src/math.ts",
            reviewer_feedback: "",
        };
        assert!(!is_python_scope(&ts));
    }

    #[test]
    fn is_python_scope_falls_back_to_a_content_vote() {
        let py_chunks = vec![py_chunk()];
        let ctx = PromptContext {
            project_name: "calc",
            project_summary: "",
            chunks: &py_chunks,
            scope_hint: "math helpers",
            reviewer_feedback: "",
        };
        assert!(is_python_scope(&ctx));

        let js_chunks = vec![Chunk {
            kind: ChunkKind::Function,
            name: "add".to_string(),
            start_line: 1,
            end_line: 1,
            content: "export const add = (a, b) => a + b;\n".to_string(),
            token_count: 8,
            oversize: false,
        }];
        let ctx = PromptContext {
            project_name: "calc",
            project_summary: "",
            chunks: &js_chunks,
            scope_hint: "math helpers",
            reviewer_feedback: "",
        };
        assert!(!is_python_scope(&ctx));

        // No signal at all → default to the JS/TS path (pre-Python behavior).
        let empty = PromptContext {
            project_name: "calc",
            project_summary: "",
            chunks: &[],
            scope_hint: "",
            reviewer_feedback: "",
        };
        assert!(!is_python_scope(&empty));
    }

    #[test]
    fn python_scope_swaps_in_the_pytest_instruction() {
        let chunks = vec![py_chunk()];
        let pc = PromptContext {
            project_name: "calc",
            project_summary: "Adder library.",
            chunks: &chunks,
            scope_hint: "src/add.py",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&pc);
        if let Content::Text { text } = &msgs[0].content[0] {
            assert!(text.contains("pytest"), "system must mandate pytest");
            assert!(text.contains("test_tc_login_01__rejects_empty_password"));
            assert!(text.contains("standard library"), "stdlib-only constraint");
            assert!(!text.contains("vitest"), "no vitest leakage on the python path");
        }
        if let Content::Text { text } = &msgs[1].content[0] {
            assert!(text.contains("test_<module>.py"));
            assert!(text.contains("double underscore"));
            assert!(!text.contains("vitest"));
        }
    }

    #[test]
    fn python_naming_convention_round_trips_through_the_case_id_regex() {
        // The prompt's example function name, transformed the way
        // `docker_py::pytest_display_name` does it, must satisfy the
        // `^TC-[A-Z0-9_-]+$` id pattern the schema enforces.
        let function = "test_tc_login_01__rejects_empty_password";
        let rest = function.strip_prefix("test_").expect("prefix");
        let (id_part, _) = rest.split_once("__").expect("separator");
        let case_id = id_part.to_ascii_uppercase().replace('_', "-");
        assert_eq!(case_id, "TC-LOGIN-01");
        assert!(case_id
            .strip_prefix("TC-")
            .expect("TC- prefix")
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '_'));
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
