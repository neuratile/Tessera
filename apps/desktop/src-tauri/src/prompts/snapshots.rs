//! Snapshot tests for prompt-template stability.
//!
//! Per `rules.md` §12.1: prompts are versioned. A silent edit to a
//! `_v1` system prompt or tool schema is a bug — bump the version
//! instead. These tests use `insta` to lock the system instructions
//! and tool schemas; an accidental wording change forces a snapshot
//! review (`cargo insta review`) before merging.
//!
//! Snapshots live next to this file under `snapshots/`. Reviewers
//! diff the YAML to confirm the change is intentional, and either
//! accept (`cargo insta accept`) or reject and fix the prompt.

#![cfg(test)]

use super::{
    bug_report_v1, bug_report_v2, context_md_v1, defect_report_v1, defect_report_v2,
    test_cases_v1, test_cases_v2, test_plan_v1, test_plan_v2, PromptContext,
};
use crate::providers::llm::types::Content;
use crate::services::chunking_service::{Chunk, ChunkKind};

fn fixture_chunks() -> Vec<Chunk> {
    vec![
        Chunk {
            kind: ChunkKind::Function,
            name: "login".to_string(),
            start_line: 10,
            end_line: 25,
            content: "function login(creds) { return verify(creds); }\n".to_string(),
            token_count: 12,
            oversize: false,
        },
        Chunk {
            kind: ChunkKind::Class,
            name: "SessionStore".to_string(),
            start_line: 30,
            end_line: 60,
            content: "class SessionStore { /* ... */ }\n".to_string(),
            token_count: 8,
            oversize: false,
        },
    ]
}

fn fixture_ctx(chunks: &[Chunk]) -> PromptContext<'_> {
    PromptContext {
        project_name: "fixture-project",
        project_summary: "Fixture summary used for snapshot tests.",
        chunks,
        scope_hint: "auth module",
        reviewer_feedback: "",
    }
}

/// Render a `Vec<Message>` to a stable YAML-ready structure for
/// snapshot comparison. Keeps the serialization decoupled from
/// whatever shape the message types happen to expose internally.
fn dump_messages(msgs: &[crate::providers::llm::types::Message]) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = msgs
        .iter()
        .map(|m| {
            let role = match m.role {
                crate::providers::llm::types::Role::System => "system",
                crate::providers::llm::types::Role::User => "user",
                crate::providers::llm::types::Role::Assistant => "assistant",
                crate::providers::llm::types::Role::Tool => "tool",
            };
            let body: String = m
                .content
                .iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::json!({
                "role": role,
                "text": body,
            })
        })
        .collect();
    serde_json::Value::Array(arr)
}

#[test]
fn snapshot_context_md_v1_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = context_md_v1::build_messages(&ctx);
    insta::assert_yaml_snapshot!("context_md_v1_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_context_md_v1_tool() {
    insta::assert_yaml_snapshot!(
        "context_md_v1_tool",
        context_md_v1::tool().parameters_schema
    );
}

#[test]
fn snapshot_test_plan_v1_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = test_plan_v1::build_messages(&ctx);
    insta::assert_yaml_snapshot!("test_plan_v1_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_test_plan_v1_tool() {
    insta::assert_yaml_snapshot!("test_plan_v1_tool", test_plan_v1::tool().parameters_schema);
}

#[test]
fn snapshot_test_cases_v1_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = test_cases_v1::build_messages(&ctx);
    insta::assert_yaml_snapshot!("test_cases_v1_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_test_cases_v1_tool() {
    insta::assert_yaml_snapshot!(
        "test_cases_v1_tool",
        test_cases_v1::tool().parameters_schema
    );
}

#[test]
fn snapshot_defect_report_v1_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = defect_report_v1::build_messages(&ctx);
    insta::assert_yaml_snapshot!("defect_report_v1_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_defect_report_v1_tool() {
    insta::assert_yaml_snapshot!(
        "defect_report_v1_tool",
        defect_report_v1::tool().parameters_schema
    );
}

#[test]
fn snapshot_test_plan_v2_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = test_plan_v2::build_messages(&ctx);
    insta::assert_yaml_snapshot!("test_plan_v2_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_test_plan_v2_tool() {
    insta::assert_yaml_snapshot!("test_plan_v2_tool", test_plan_v2::tool().parameters_schema);
}

#[test]
fn snapshot_defect_report_v2_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = defect_report_v2::build_messages(&ctx);
    insta::assert_yaml_snapshot!("defect_report_v2_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_defect_report_v2_tool() {
    insta::assert_yaml_snapshot!(
        "defect_report_v2_tool",
        defect_report_v2::tool().parameters_schema
    );
}

#[test]
fn snapshot_test_cases_v2_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = test_cases_v2::build_messages(&ctx);
    insta::assert_yaml_snapshot!("test_cases_v2_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_test_cases_v2_tool() {
    insta::assert_yaml_snapshot!(
        "test_cases_v2_tool",
        test_cases_v2::tool().parameters_schema
    );
}

#[test]
fn snapshot_bug_report_v2_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = bug_report_v2::build_messages(&ctx);
    insta::assert_yaml_snapshot!("bug_report_v2_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_bug_report_v2_tool() {
    insta::assert_yaml_snapshot!(
        "bug_report_v2_tool",
        bug_report_v2::tool().parameters_schema
    );
}

#[test]
fn snapshot_bug_report_v1_messages() {
    let chunks = fixture_chunks();
    let ctx = fixture_ctx(&chunks);
    let msgs = bug_report_v1::build_messages(&ctx);
    insta::assert_yaml_snapshot!("bug_report_v1_messages", dump_messages(&msgs));
}

#[test]
fn snapshot_bug_report_v1_tool() {
    insta::assert_yaml_snapshot!(
        "bug_report_v1_tool",
        bug_report_v1::tool().parameters_schema
    );
}
