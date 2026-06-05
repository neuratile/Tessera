//! Prompt template: project `context.md` generation, version 1.
//!
//! Bottom-up summarization: given a sample of representative chunks
//! from the project (typically: top-level entry points, public
//! exports, type definitions), the model produces a structured
//! Markdown overview that downstream artifact prompts inline as
//! `PromptContext::project_summary`.
//!
//! Output shape: `emit_project_context` tool call with `summary`,
//! `architecture_notes`, `key_modules`, `data_flows`, and
//! `known_risks` fields. Free-form prose is wrapped in deliberate
//! sections so later prompts can quote them.

use std::fmt::Write as _;

use crate::providers::llm::types::{Message, ToolSchema};

use super::{system_text, tool_schema, user_text, PromptContext};

/// Stable version identifier persisted alongside any artifact this
/// prompt produces (rules.md §12.1).
pub const VERSION: &str = "context_md_v1";

/// JSON-Schema function name the model must invoke.
pub const TOOL_NAME: &str = "emit_project_context";

const SYSTEM_INSTRUCTIONS: &str = "\
You are a senior software architect summarizing a codebase for downstream \
test-strategy generation. Read the supplied chunks and produce a structured \
overview.

Rules:
- Cite specific files / functions / classes by name when describing \
  architecture. Do NOT invent symbols that do not appear in the chunks.
- Distinguish what the code OBVIOUSLY does from what you are INFERRING.
- Flag uncertainty explicitly — write 'unclear from sampled chunks' rather \
  than guessing.
- Output is consumed by other LLM prompts, so prefer stable noun-phrase \
  references over narrative paragraphs.
- For string fields in the schema (like `summary` and `architecture_notes`), \
  you MUST supply a single string. Format any bullet lists as markdown inside \
  a single string (e.g. '* item1\\n* item2'), NEVER as a JSON array of strings.
- Always invoke the `emit_project_context` tool with the structured payload. \
  Never reply with free-form prose.

The structured payload MUST have the following JSON structure:
{
  \"summary\": \"One-paragraph elevator pitch for the project\",
  \"architecture_notes\": \"Bullet-prose describing layers, boundaries, and data flow. MUST be a single string (using markdown bullet points like '* text\\n* text'), NOT a JSON array of strings.\",
  \"key_modules\": [
    {
      \"name\": \"Module/file name\",
      \"responsibility\": \"Core module responsibility\"
    }
  ],
  \"data_flows\": [
    {
      \"producer\": \"Producer name\",
      \"consumer\": \"Consumer name\",
      \"payload\": \"Payload description\"
    }
  ],
  \"known_risks\": [\"Risk description\"]
}";

/// Build the message sequence for a context-summarization request.
#[must_use]
pub fn build_messages(ctx: &PromptContext<'_>) -> Vec<Message> {
    let mut user_body = String::new();
    writeln!(user_body, "# Project: {}\n", ctx.project_name).expect("write");

    if !ctx.scope_hint.is_empty() {
        writeln!(user_body, "Scope hint: {}\n", ctx.scope_hint).expect("write");
    }

    if !ctx.reviewer_feedback.is_empty() {
        user_body.push_str("## Previous reviewer feedback\n\n");
        user_body.push_str(ctx.reviewer_feedback);
        user_body.push_str("\n\n");
    }

    user_body.push_str("## Sampled chunks\n\n");
    user_body.push_str(&ctx.render_chunks());
    user_body.push_str("\n\n[CRITICAL INSTRUCTION] You MUST now invoke `emit_project_context` with the structured summary.\n\
    The JSON payload MUST have exactly these keys (and no others):\n\
    {\n\
      \"summary\": \"One-paragraph elevator pitch for the project\",\n\
      \"architecture_notes\": \"Bullet-prose describing layers, boundaries, and data flow. MUST be a single string (using markdown bullet points like '* text\\n* text'), NOT a JSON array of strings.\",\n\
      \"key_modules\": [\n\
        {\n\
          \"name\": \"Module/file name\",\n\
          \"responsibility\": \"Core module responsibility\"\n\
        }\n\
      ]\n\
    }\n\
    Do NOT output fields from the codebase (like architect, location, timeline, bhk_display, category, etc.) at the top level. You MUST use only the keys listed above. Do NOT reply with free-form prose, apologies, or explanations. You MUST invoke the tool.");

    vec![system_text(SYSTEM_INSTRUCTIONS), user_text(user_body)]
}

/// JSON-Schema tool definition. Matches the `ProjectContext` Zod
/// schema in `packages/shared/` (rules.md §12.3.1 — Rust is the
/// source of truth; the Zod schema mirrors).
#[must_use]
pub fn tool() -> ToolSchema {
    tool_schema(
        TOOL_NAME,
        "Emit a structured project-context summary for downstream test-artifact prompts.",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["summary", "architecture_notes", "key_modules"],
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "One-paragraph elevator pitch for the project.",
                    "minLength": 1,
                    "maxLength": 1200
                },
                "architecture_notes": {
                    "type": "string",
                    "description": "Bullet-prose describing layers, boundaries, and data flow. \
                                   Cite files / functions / classes by name. MUST be a single string \
                                   (using markdown bullet points like '* text\\n* text'), NOT a JSON array of strings.",
                    "minLength": 1
                },
                "key_modules": {
                    "type": "array",
                    "description": "Top 3-10 modules / files that drive the system's behavior.",
                    "maxItems": 20,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["name", "responsibility"],
                        "properties": {
                            "name": { "type": "string", "minLength": 1 },
                            "responsibility": { "type": "string", "minLength": 1 }
                        }
                    }
                },
                "data_flows": {
                    "type": "array",
                    "description": "Notable producer -> consumer flows the model identified.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["producer", "consumer", "payload"],
                        "properties": {
                            "producer": { "type": "string", "minLength": 1 },
                            "consumer": { "type": "string", "minLength": 1 },
                            "payload": { "type": "string", "minLength": 1 }
                        }
                    }
                },
                "known_risks": {
                    "type": "array",
                    "description": "Areas the model could not understand from sampled chunks. \
                                   Empty when nothing is unclear.",
                    "items": { "type": "string", "minLength": 1 }
                }
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::llm::types::Role;
    use crate::services::chunking_service::{Chunk, ChunkKind};

    fn fixture_ctx() -> (Vec<Chunk>, &'static str) {
        let chunks = vec![Chunk {
            kind: ChunkKind::Function,
            name: "main".to_string(),
            start_line: 1,
            end_line: 3,
            content: "fn main() { println!(\"hi\"); }\n".to_string(),
            token_count: 7,
            oversize: false,
        }];
        let summary = "Hello-world rust binary.";
        (chunks, summary)
    }

    #[test]
    fn build_messages_emits_system_then_user() {
        let (chunks, summary) = fixture_ctx();
        let ctx = PromptContext {
            project_name: "demo",
            project_summary: summary,
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&ctx);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn user_message_carries_project_name_and_chunk() {
        let (chunks, summary) = fixture_ctx();
        let ctx = PromptContext {
            project_name: "demo",
            project_summary: summary,
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&ctx);
        let user_text = match &msgs[1].content[0] {
            crate::providers::llm::types::Content::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(user_text.contains("# Project: demo"));
        assert!(user_text.contains("function `main`"));
        assert!(user_text.contains("invoke `emit_project_context`"));
    }

    #[test]
    fn user_message_includes_scope_hint_when_present() {
        let (chunks, summary) = fixture_ctx();
        let ctx = PromptContext {
            project_name: "demo",
            project_summary: summary,
            chunks: &chunks,
            scope_hint: "src/payments/",
            reviewer_feedback: "",
        };
        let msgs = build_messages(&ctx);
        let user_text = match &msgs[1].content[0] {
            crate::providers::llm::types::Content::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(user_text.contains("Scope hint: src/payments/"));
    }

    #[test]
    fn user_message_includes_reviewer_feedback_when_present() {
        let (chunks, summary) = fixture_ctx();
        let ctx = PromptContext {
            project_name: "demo",
            project_summary: summary,
            chunks: &chunks,
            scope_hint: "",
            reviewer_feedback: "Last pass missed payments.",
        };
        let msgs = build_messages(&ctx);
        let user_text = match &msgs[1].content[0] {
            crate::providers::llm::types::Content::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(user_text.contains("Previous reviewer feedback"));
        assert!(user_text.contains("Last pass missed payments."));
    }

    #[test]
    fn tool_schema_advertises_required_fields() {
        let schema = tool();
        assert_eq!(schema.name, TOOL_NAME);
        let required = schema.parameters_schema["required"]
            .as_array()
            .expect("required");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"summary"));
        assert!(names.contains(&"architecture_notes"));
        assert!(names.contains(&"key_modules"));
    }

    #[test]
    fn version_constant_matches_filename() {
        assert_eq!(VERSION, "context_md_v1");
    }

    #[test]
    fn system_instructions_contain_no_hallucinate_directive() {
        // rules.md §12.4: never silently retry or hallucinate. The
        // instruction wording is deliberate; this test catches an
        // accidental edit to the system prompt.
        assert!(SYSTEM_INSTRUCTIONS.contains("Do NOT invent symbols"));
        assert!(SYSTEM_INSTRUCTIONS.contains("Flag uncertainty explicitly"));
    }
}
