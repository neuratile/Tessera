//! Prompt template: runnable `files[]` repair pass, version 1.
//!
//! Some models (observed with Gemini via the OpenAI-compat surface and
//! small Ollama models) emit a valid `cases` payload for
//! `emit_test_cases` but skip the *optional* `files` array — leaving
//! the artifact descriptive-only, so the sandbox runner rejects it
//! with "artifact has no runnable test files". This focused follow-up
//! prompt feeds the already-validated cases plus the original code
//! chunks back to the model and asks for **only** the runnable
//! workspace. Here `files` is required (`minItems: 1`) — the item
//! schema is byte-identical to the `emit_test_cases` contract
//! ([`runnable_files_schema`]) so the merged payload still validates
//! against the test-cases tool schema.

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{runnable_files_schema, system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "runnable_files_v1";

pub const TOOL_NAME: &str = "emit_runnable_files";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior test engineer. You are given an existing set of \
structured test cases and the source code they cover. Your ONLY job is \
to emit the runnable workspace that makes those cases executable in a \
local vitest sandbox.

Rules:
- Emit a `files` array containing the minimal source-under-test \
  file(s), reproduced faithfully from the supplied chunks and marked \
  `isTest: false`, plus one vitest spec per source file marked \
  `isTest: true`.
- Each spec uses `import { describe, it, expect } from 'vitest'` and \
  imports the source by relative path. One `it(...)` per test case \
  where practical, asserting that case's expected results.
- Use workspace-relative paths only — never an absolute path or a \
  `..` segment.
- Do NOT restate, modify, or re-emit the test cases themselves. Only \
  the `files` array.
- Always invoke the `emit_runnable_files` tool with the structured \
  payload. Never reply with free-form prose.";

#[must_use]
pub fn build_messages(ctx: &PromptContext<'_>, cases_json: &str) -> Vec<Message> {
    let mut user_body = String::new();
    writeln!(user_body, "# Project: {}\n", ctx.project_name).expect("write");

    if !ctx.scope_hint.is_empty() {
        writeln!(user_body, "Scope: {}\n", ctx.scope_hint).expect("write");
    }

    user_body.push_str("## Existing test cases (already final — do not change)\n\n");
    user_body.push_str(cases_json);
    user_body.push_str("\n\n## Source code under test\n\n");
    user_body.push_str(&ctx.render_chunks());
    user_body.push_str(
        "\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_runnable_files` tool. \
         The JSON payload MUST use ONLY the top-level key `files` (required, at least one entry):\n\
         {\n\
           \"files\": [\n\
             { \"path\": \"src/add.ts\", \"contents\": \"...\", \"isTest\": false },\n\
             { \"path\": \"add.test.ts\", \"contents\": \"...\", \"isTest\": true }\n\
           ]\n\
         }\n\
         Reproduce the source-under-test from the chunks above (isTest:false) and write one \
         vitest spec per source file (isTest:true) covering the supplied cases. Use \
         workspace-relative paths only (no absolute paths, no `..`). Do NOT reply with \
         free-form prose, apologies, or explanations. You MUST invoke the tool.",
    );

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let mut files = runnable_files_schema();
    if let Some(obj) = files.as_object_mut() {
        // Same item contract as `emit_test_cases`; only the array-level
        // constraints differ — here the workspace is mandatory.
        obj.insert("minItems".to_string(), serde_json::json!(1));
        obj.insert(
            "description".to_string(),
            serde_json::json!(
                "Runnable workspace mirroring the supplied cases: minimal source-under-test plus generated vitest specs. Required — at least one source file and one spec."
            ),
        );
    }
    tool_schema(
        TOOL_NAME,
        "Emit the runnable vitest workspace for an existing set of test cases.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["files"],
            "properties": {
                "files": files
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
        assert_eq!(VERSION, "runnable_files_v1");
    }

    #[test]
    fn files_are_required_with_at_least_one_entry() {
        let schema = tool();
        let required: Vec<&str> = schema.parameters_schema["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(required, vec!["files"]);
        assert_eq!(
            schema.parameters_schema["properties"]["files"]["minItems"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn files_item_contract_matches_test_cases_schema() {
        // The sandbox runner contract must not drift: the repair tool's
        // `files.items` is the same JSON value as `emit_test_cases`'.
        let cases_tool = super::super::test_cases_v2::tool();
        let repair_tool = tool();
        assert_eq!(
            cases_tool.parameters_schema["properties"]["files"]["items"],
            repair_tool.parameters_schema["properties"]["files"]["items"]
        );
    }

    #[test]
    fn build_messages_embeds_cases_and_chunks() {
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
        let cases_json = r#"[{"id":"TC-ADD-1"}]"#;
        let msgs = build_messages(&pc, cases_json);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
        if let Content::Text { text } = &msgs[1].content[0] {
            assert!(text.contains("TC-ADD-1"));
            assert!(text.contains("function `add`"));
            assert!(text.contains("emit_runnable_files"));
        } else {
            panic!("expected text content");
        }
    }
}
