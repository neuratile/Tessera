//! Prompt template: static-analysis defect report, version 2.
//!
//! Industry-grade upgrade over `defect_report_v1` (`plan/ARTIFACT_QUALITY_V2.md`
//! Phase 2): `category` aligned to CWE top classes, required
//! `fixSuggestion` per finding, and evidence fields at parity with the
//! bug report (`evidenceSnippet`, `location.fileHint` + line range).
//! Field names are camelCase to match the shared Zod mirror
//! (`packages/shared/src/schemas/defect-report.schema.ts`).

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

pub const VERSION: &str = "defect_report_v2";

pub const TOOL_NAME: &str = "emit_defect_report";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior code reviewer running a static-analysis pass. The output \
ships to a human triage queue; precision matters more than recall.

Rules:
- Only report findings with HIGH or MEDIUM confidence. Do NOT pad with \
  speculative low-confidence noise.
- Cite a specific symbol + line range from the supplied chunks for every \
  finding. If the location is not visible in the chunks, do NOT report it.
- `category` follows CWE top classes: input_validation (CWE-20), auth \
  (CWE-287/862), resource_management (CWE-400/401), logic (CWE-840), \
  error_handling (CWE-755), concurrency (CWE-362).
- `fixSuggestion` must be a concrete code change, not 'consider \
  refactoring'.
- `evidenceSnippet` quotes the offending code verbatim from the supplied \
  chunks — never paraphrase.
- Defect IDs must strictly match the regex `^DEF-[A-Z0-9_-]+$` (all-caps, \
  e.g., `DEF-NULL-POINTER`, NOT `DEF-Null-Pointer` or `DEF-Null`).
- Always invoke the `emit_defect_report` tool with the structured payload. \
  Never reply with free-form prose.

The structured payload MUST have the following JSON structure:
{
  \"findings\": [
    {
      \"id\": \"DEF-UNIQUE-ID\",
      \"severity\": \"critical | major | minor | trivial\",
      \"category\": \"input_validation | auth | resource_management | logic | error_handling | concurrency\",
      \"confidence\": \"high | medium\",
      \"location\": {
        \"symbol\": \"Function/class/method name\",
        \"startLine\": 10,
        \"endLine\": 20,
        \"fileHint\": \"path/to/file.ext\"
      },
      \"description\": \"Detailed description of the defect\",
      \"impact\": \"Potential impact of the defect\",
      \"fixSuggestion\": \"Concrete code fix suggestion\",
      \"evidenceSnippet\": \"Verbatim snippet of the offending code\"
    }
  ],
  \"summary\": \"One-paragraph overview of findings\"
}";

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
    user_body.push_str("\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_defect_report` tool with the structured findings.\n\
    The JSON payload MUST have exactly these keys (and no others):\n\
    {\n\
      \"findings\": [\n\
        {\n\
          \"id\": \"DEF-UNIQUE-ID\",\n\
          \"severity\": \"critical | major | minor | trivial\",\n\
          \"category\": \"input_validation | auth | resource_management | logic | error_handling | concurrency\",\n\
          \"confidence\": \"high | medium\",\n\
          \"location\": {\n\
            \"symbol\": \"Function/class/method name\",\n\
            \"startLine\": 1,\n\
            \"endLine\": 10,\n\
            \"fileHint\": \"path/to/file.ext\"\n\
          },\n\
          \"description\": \"Detailed description of the defect\",\n\
          \"impact\": \"Potential impact of the defect\",\n\
          \"fixSuggestion\": \"Concrete code fix suggestion\",\n\
          \"evidenceSnippet\": \"Verbatim snippet of the offending code\"\n\
        }\n\
      ],\n\
      \"summary\": \"One-paragraph overview of findings\"\n\
    }\n\
    `location` is a nested OBJECT — never emit symbol/startLine/endLine/fileHint at the finding level. `evidenceSnippet` quotes the offending code verbatim.\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let severity_enum = serde_json::json!(["critical", "major", "minor", "trivial"]);
    let confidence_enum = serde_json::json!(["high", "medium"]);
    let category_enum = serde_json::json!([
        "input_validation",
        "auth",
        "resource_management",
        "logic",
        "error_handling",
        "concurrency"
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
                            "fixSuggestion"
                        ],
                        "properties": {
                            "id": {
                                "type": "string",
                                "pattern": "^DEF-[A-Z0-9_-]+$",
                                "description": "Stable id, prefix `DEF-`. MUST use ONLY uppercase letters, digits, hyphens, and underscores (e.g. 'DEF-NULL-POINTER' in all-caps, NOT 'DEF-Null-Pointer')."
                            },
                            "severity": { "type": "string", "enum": severity_enum },
                            "category": {
                                "type": "string",
                                "enum": category_enum,
                                "description": "CWE top-class alignment: input_validation (CWE-20), auth (CWE-287/862), resource_management (CWE-400/401), logic (CWE-840), error_handling (CWE-755), concurrency (CWE-362)."
                            },
                            "confidence": { "type": "string", "enum": confidence_enum },
                            "location": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["symbol", "startLine", "endLine"],
                                "properties": {
                                    "symbol": { "type": "string", "minLength": 1 },
                                    "startLine": { "type": "integer", "minimum": 1 },
                                    "endLine": { "type": "integer", "minimum": 1 },
                                    "fileHint": { "type": "string" }
                                }
                            },
                            "description": { "type": "string", "minLength": 10 },
                            "impact": { "type": "string", "minLength": 5 },
                            "fixSuggestion": { "type": "string", "minLength": 5 },
                            "evidenceSnippet": {
                                "type": "string",
                                "description": "Short verbatim quote of the offending code."
                            }
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
        assert_eq!(VERSION, "defect_report_v2");
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
    fn category_enum_is_cwe_aligned_six_buckets() {
        let schema = tool();
        let cats: Vec<&str> = schema.parameters_schema["properties"]["findings"]["items"]
            ["properties"]["category"]["enum"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            cats,
            vec![
                "input_validation",
                "auth",
                "resource_management",
                "logic",
                "error_handling",
                "concurrency"
            ]
        );
    }

    #[test]
    fn fix_suggestion_is_required_per_finding() {
        let schema = tool();
        let required: Vec<&str> = schema.parameters_schema["properties"]["findings"]["items"]
            ["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"fixSuggestion"));
        // v1's snake_case suggested_fix is gone.
        let props = schema.parameters_schema["properties"]["findings"]["items"]["properties"]
            .as_object()
            .expect("object");
        assert!(!props.contains_key("suggested_fix"));
    }

    #[test]
    fn evidence_fields_match_bug_report_parity() {
        // Plan Phase 2: evidence parity with the bug report — verbatim
        // snippet plus fileHint + line range on the location object.
        let schema = tool();
        let props = &schema.parameters_schema["properties"]["findings"]["items"]["properties"];
        assert!(props.get("evidenceSnippet").is_some());
        let loc_props = props["location"]["properties"]
            .as_object()
            .expect("object");
        assert!(loc_props.contains_key("fileHint"));
        assert!(loc_props.contains_key("startLine"));
        assert!(loc_props.contains_key("endLine"));
    }

    #[test]
    fn confidence_enum_is_high_or_medium_only() {
        let schema = tool();
        let v: Vec<&str> = schema.parameters_schema["properties"]["findings"]["items"]
            ["properties"]["confidence"]["enum"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(v, vec!["high", "medium"]);
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
}
