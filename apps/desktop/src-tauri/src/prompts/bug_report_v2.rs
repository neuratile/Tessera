//! Prompt template: bug report (runtime issue), version 2.
//!
//! Industry-grade upgrade over `bug_report_v1` (`plan/ARTIFACT_QUALITY_V2.md`
//! Phase 1), aligned with ISTQB / issue-tracker conventions: severity
//! (5-level, impact) split from priority (urgency), `reproducibility`,
//! optional `workaround` and `component`, and `stepsToReproduce` with
//! `minItems: 1`. Field names are camelCase to match the shared Zod
//! mirror (`packages/shared/src/schemas/bug-report.schema.ts`).

use crate::providers::llm::types::{Message, ToolSchema};

use super::{
    render_user_body, system_text, tool_schema, user_text, PromptContext, UserBodyOptions,
};

pub const VERSION: &str = "bug_report_v2";

pub const TOOL_NAME: &str = "emit_bug_report";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior engineer formatting a runtime-issue report for an issue \
tracker. The output must be self-contained — a triage engineer should \
understand the problem without reading the surrounding context.

Rules:
- Steps to reproduce are imperative, ordered, numbered, and minimal — \
  strip irrelevant setup. At least one step is required.
- Expected vs Actual must be concrete and observable.
- `severity` is IMPACT on the system (blocker > critical > major > minor \
  > trivial). `priority` is URGENCY of the fix (p0 > p1 > p2 > p3). They \
  are independent — a cosmetic blocker on a rarely-used screen can be \
  severity blocker / priority p3.
- `reproducibility` states how reliably the bug occurs: always, \
  intermittent, or once.
- Supply `workaround` when a user-side mitigation exists; supply \
  `component` (module / subsystem name) when the affected area is clear \
  from the code.
- Root cause analysis cites specific symbols / line ranges from the \
  supplied chunks. Do NOT speculate beyond what the code shows.
- One bug per report. If multiple defects share a symptom, emit \
  multiple reports rather than coalescing.
- Bug IDs must strictly match the regex `^BUG-[A-Z0-9_-]+$` (all-caps, \
  e.g., `BUG-SESSION-LEAK`, NOT `BUG-Session-Leak` or `BUG-Session`).
- Always invoke the `emit_bug_report` tool with the structured payload. \
  Never reply with free-form prose.

The structured payload MUST have the following JSON structure:
{
  \"bugs\": [
    {
      \"id\": \"BUG-UNIQUE-ID\",
      \"title\": \"Short descriptive title of the bug\",
      \"severity\": \"blocker | critical | major | minor | trivial\",
      \"priority\": \"p0 | p1 | p2 | p3\",
      \"reproducibility\": \"always | intermittent | once\",
      \"environment\": \"OS/runtime stack when known\",
      \"component\": \"Affected module / subsystem when known\",
      \"stepsToReproduce\": [\"1. Step one\", \"2. Step two\"],
      \"expectedBehavior\": \"Expected behavior\",
      \"actualBehavior\": \"Actual behavior\",
      \"workaround\": \"User-side mitigation when one exists\",
      \"rootCause\": {
        \"symbol\": \"Function/class/method name\",
        \"startLine\": 10,
        \"endLine\": 20,
        \"fileHint\": \"path/to/file.ext\",
        \"explanation\": \"Root cause explanation\"
      },
      \"evidenceSnippet\": \"Verbatim snippet of code showing the bug\"
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
            feedback_heading: "Reviewer feedback / observed symptoms",
            code_heading: "Code in scope",
        },
    );
    user_body.push_str("\n\n[CRITICAL INSTRUCTION] You MUST now invoke the `emit_bug_report` tool with the structured report.\n\
    The JSON payload MUST have exactly these keys (and no others):\n\
    {\n\
      \"bugs\": [\n\
        {\n\
          \"id\": \"BUG-UNIQUE-ID\",\n\
          \"title\": \"Short descriptive title of the bug\",\n\
          \"severity\": \"blocker | critical | major | minor | trivial\",\n\
          \"priority\": \"p0 | p1 | p2 | p3\",\n\
          \"reproducibility\": \"always | intermittent | once\",\n\
          \"environment\": \"OS/runtime stack when known\",\n\
          \"component\": \"Affected module / subsystem when known\",\n\
          \"stepsToReproduce\": [\"1. Step one\", \"2. Step two\"],\n\
          \"expectedBehavior\": \"Expected behavior\",\n\
          \"actualBehavior\": \"Actual behavior\",\n\
          \"workaround\": \"User-side mitigation when one exists\",\n\
          \"rootCause\": {\n\
            \"symbol\": \"Function/class/method name\",\n\
            \"startLine\": 1,\n\
            \"endLine\": 10,\n\
            \"fileHint\": \"path/to/file.ext\",\n\
            \"explanation\": \"Root cause explanation\"\n\
          },\n\
          \"evidenceSnippet\": \"Verbatim snippet of code showing the bug\"\n\
        }\n\
      ]\n\
    }\n\
    `severity` is impact (blocker|critical|major|minor|trivial); `priority` is urgency (p0|p1|p2|p3) — set them independently. `stepsToReproduce` needs at least one numbered step.\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

#[must_use]
pub fn tool() -> ToolSchema {
    let severity_enum = serde_json::json!(["blocker", "critical", "major", "minor", "trivial"]);
    let priority_enum = serde_json::json!(["p0", "p1", "p2", "p3"]);
    let reproducibility_enum = serde_json::json!(["always", "intermittent", "once"]);

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
                            "priority",
                            "reproducibility",
                            "stepsToReproduce",
                            "expectedBehavior",
                            "actualBehavior",
                            "rootCause"
                        ],
                        "properties": {
                            "id": {
                                "type": "string",
                                "pattern": "^BUG-[A-Z0-9_-]+$",
                                "description": "Stable id, prefix `BUG-`. MUST use ONLY uppercase letters, digits, hyphens, and underscores (e.g. 'BUG-SESSION-LEAK' in all-caps, NOT 'BUG-Session-Leak')."
                            },
                            "title": { "type": "string", "minLength": 10, "maxLength": 200 },
                            "severity": {
                                "type": "string",
                                "enum": severity_enum,
                                "description": "Impact on the system, independent of fix urgency."
                            },
                            "priority": {
                                "type": "string",
                                "enum": priority_enum,
                                "description": "Urgency of the fix, independent of impact."
                            },
                            "reproducibility": {
                                "type": "string",
                                "enum": reproducibility_enum,
                                "description": "How reliably the bug occurs."
                            },
                            "environment": {
                                "type": "string",
                                "description": "OS / runtime / version stack when known."
                            },
                            "component": {
                                "type": "string",
                                "description": "Affected module / subsystem when known."
                            },
                            "stepsToReproduce": {
                                "type": "array",
                                "minItems": 1,
                                "items": { "type": "string", "minLength": 1 }
                            },
                            "expectedBehavior": { "type": "string", "minLength": 1 },
                            "actualBehavior": { "type": "string", "minLength": 1 },
                            "workaround": {
                                "type": "string",
                                "description": "User-side mitigation when one exists."
                            },
                            "rootCause": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["symbol", "explanation"],
                                "properties": {
                                    "symbol": { "type": "string", "minLength": 1 },
                                    "startLine": { "type": "integer", "minimum": 1 },
                                    "endLine": { "type": "integer", "minimum": 1 },
                                    "fileHint": { "type": "string" },
                                    "explanation": { "type": "string", "minLength": 10 }
                                }
                            },
                            "evidenceSnippet": {
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
        assert_eq!(VERSION, "bug_report_v2");
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
    fn severity_enum_is_five_levels() {
        let schema = tool();
        let sev = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"]
            ["severity"]["enum"];
        let v: Vec<&str> = sev
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(v, vec!["blocker", "critical", "major", "minor", "trivial"]);
    }

    #[test]
    fn priority_is_split_from_severity() {
        let schema = tool();
        let props = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"];
        let pri: Vec<&str> = props["priority"]["enum"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(pri, vec!["p0", "p1", "p2", "p3"]);

        let required: Vec<&str> = schema.parameters_schema["properties"]["bugs"]["items"]
            ["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"severity"));
        assert!(required.contains(&"priority"));
    }

    #[test]
    fn reproducibility_enum_and_optional_triage_fields() {
        let schema = tool();
        let props = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"];
        let rep: Vec<&str> = props["reproducibility"]["enum"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(rep, vec!["always", "intermittent", "once"]);

        let required: Vec<&str> = schema.parameters_schema["properties"]["bugs"]["items"]
            ["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"reproducibility"));
        // workaround + component are offered but optional.
        assert!(!required.contains(&"workaround"));
        assert!(!required.contains(&"component"));
        assert!(props.get("workaround").is_some());
        assert!(props.get("component").is_some());
    }

    #[test]
    fn steps_to_reproduce_requires_at_least_one_step() {
        let schema = tool();
        let steps = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"]
            ["stepsToReproduce"];
        assert_eq!(steps["minItems"].as_u64(), Some(1));

        let required: Vec<&str> = schema.parameters_schema["properties"]["bugs"]["items"]
            ["required"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"stepsToReproduce"));
    }

    #[test]
    fn root_cause_requires_symbol_and_explanation() {
        let schema = tool();
        let req = &schema.parameters_schema["properties"]["bugs"]["items"]["properties"]
            ["rootCause"]["required"];
        let v: Vec<&str> = req
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(v.contains(&"symbol"));
        assert!(v.contains(&"explanation"));
    }
}
