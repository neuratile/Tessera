//! Per-artifact-type mappers from `structured_data` JSON into the
//! export IR.
//!
//! Payload shapes live in `payload.rs` (shared with the markdown
//! writer in `markdown_writer.rs`). A fully empty payload (`null` /
//! `{}`) is rejected with [`AppError::InvalidInput`] so the frontend
//! can suggest the markdown export instead.

use crate::error::{AppError, AppResult};
use crate::repositories::artifact_repo::{Artifact, ArtifactType};

use super::ir::{
    clamp_cell, joined_lines, numbered_lines, ExportDoc, ExportSection, ExportTable,
    KeyValueSection,
};
use super::payload::{
    BugReportPayload, ContextMdPayload, DefectReportPayload, TestCasesPayload, TestPlanPayload,
    TestStep,
};

/// Build the writer-agnostic [`ExportDoc`] for an artifact. Pure —
/// this is the seam every destination (csv/tsv/xlsx, later Jira)
/// consumes.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `structured_data` is `null` or an
///   empty object — there is nothing tabular to export.
/// - [`AppError::Serde`] when the payload cannot deserialize into the
///   artifact type's expected shape.
pub fn build_export_doc(artifact: &Artifact) -> AppResult<ExportDoc> {
    let data = &artifact.structured_data;
    let is_empty_object = data.as_object().is_some_and(serde_json::Map::is_empty);
    if data.is_null() || is_empty_object {
        return Err(AppError::InvalidInput(
            "artifact has no structured data to export".into(),
        ));
    }

    let sections = match artifact.artifact_type {
        ArtifactType::TestCases => map_test_cases(data)?,
        ArtifactType::DefectReport => map_defect_report(data)?,
        ArtifactType::BugReport => map_bug_report(data)?,
        ArtifactType::TestPlan => map_test_plan(data)?,
        ArtifactType::ContextMd => map_context_md(data)?,
    };

    Ok(ExportDoc {
        title: artifact.title.clone(),
        sections,
    })
}

// ---------------------------------------------------------------------------
// test_cases (v1 + v2)
// ---------------------------------------------------------------------------

fn map_test_cases(data: &serde_json::Value) -> AppResult<Vec<ExportSection>> {
    let payload: TestCasesPayload = serde_json::from_value(data.clone())?;

    let rows = payload
        .cases
        .iter()
        .map(|case| {
            let actions: Vec<String> = case
                .steps
                .iter()
                .map(|s| match s {
                    TestStep::Separated { action, .. } => action.clone(),
                    TestStep::Plain(text) => text.clone(),
                })
                .collect();
            let per_step_results: Vec<String> = case
                .steps
                .iter()
                .filter_map(|s| match s {
                    TestStep::Separated {
                        expected_result, ..
                    } if !expected_result.is_empty() => Some(expected_result.clone()),
                    _ => None,
                })
                .collect();
            // v2 carries per-step results; v1 a single case-level one.
            let expected = if per_step_results.is_empty() {
                case.expected_result.clone()
            } else {
                numbered_lines(&per_step_results)
            };

            vec![
                clamp_cell(case.id.clone()),
                clamp_cell(case.title.clone()),
                clamp_cell(case.case_type.clone()),
                clamp_cell(case.priority.clone()),
                clamp_cell(numbered_lines(&case.preconditions)),
                clamp_cell(case.test_data.clone()),
                clamp_cell(numbered_lines(&actions)),
                clamp_cell(expected),
                clamp_cell(numbered_lines(&case.postconditions)),
                clamp_cell(joined_lines(&case.traceability)),
            ]
        })
        .collect();

    let mut sections = vec![ExportSection::Table(ExportTable {
        name: "Test Cases".into(),
        columns: [
            "ID",
            "Title",
            "Type",
            "Priority",
            "Preconditions",
            "Test Data",
            "Steps",
            "Expected Result",
            "Postconditions",
            "Traceability",
        ]
        .map(String::from)
        .to_vec(),
        rows,
    })];

    if !payload.files.is_empty() {
        let file_rows = payload
            .files
            .iter()
            .map(|f| {
                vec![
                    clamp_cell(f.path.clone()),
                    if f.is_test { "test" } else { "source" }.to_string(),
                    clamp_cell(f.contents.clone()),
                ]
            })
            .collect();
        sections.push(ExportSection::Table(ExportTable {
            name: "Files".into(),
            columns: ["Path", "Role", "Contents"].map(String::from).to_vec(),
            rows: file_rows,
        }));
    }

    Ok(sections)
}

// ---------------------------------------------------------------------------
// defect_report
// ---------------------------------------------------------------------------

fn map_defect_report(data: &serde_json::Value) -> AppResult<Vec<ExportSection>> {
    let payload: DefectReportPayload = serde_json::from_value(data.clone())?;

    let rows = payload
        .findings
        .iter()
        .map(|f| {
            vec![
                clamp_cell(f.id.clone()),
                clamp_cell(f.severity.clone()),
                clamp_cell(f.category.clone()),
                clamp_cell(f.confidence.clone()),
                clamp_cell(f.location.flatten()),
                clamp_cell(f.description.clone()),
                clamp_cell(f.impact.clone()),
                clamp_cell(f.suggested_fix.clone()),
            ]
        })
        .collect();

    let mut sections = vec![ExportSection::Table(ExportTable {
        name: "Findings".into(),
        columns: [
            "ID",
            "Severity",
            "Category",
            "Confidence",
            "Location",
            "Description",
            "Impact",
            "Suggested Fix",
        ]
        .map(String::from)
        .to_vec(),
        rows,
    })];

    if !payload.summary.is_empty() {
        sections.push(ExportSection::KeyValues(KeyValueSection {
            name: "Summary".into(),
            entries: vec![("Summary".into(), clamp_cell(payload.summary))],
        }));
    }

    Ok(sections)
}

// ---------------------------------------------------------------------------
// bug_report (v1 + v2)
// ---------------------------------------------------------------------------

fn map_bug_report(data: &serde_json::Value) -> AppResult<Vec<ExportSection>> {
    let payload: BugReportPayload = serde_json::from_value(data.clone())?;

    let rows = payload
        .bugs
        .iter()
        .map(|b| {
            vec![
                clamp_cell(b.id.clone()),
                clamp_cell(b.title.clone()),
                clamp_cell(b.severity.clone()),
                clamp_cell(b.priority.clone()),
                clamp_cell(b.reproducibility.clone()),
                clamp_cell(b.environment.clone()),
                clamp_cell(b.component.clone()),
                clamp_cell(numbered_lines(&b.steps_to_reproduce)),
                clamp_cell(b.expected_behavior.clone()),
                clamp_cell(b.actual_behavior.clone()),
                clamp_cell(b.workaround.clone()),
                clamp_cell(b.root_cause.flatten()),
                clamp_cell(b.evidence_snippet.clone()),
            ]
        })
        .collect();

    Ok(vec![ExportSection::Table(ExportTable {
        name: "Bugs".into(),
        columns: [
            "ID",
            "Title",
            "Severity",
            "Priority",
            "Reproducibility",
            "Environment",
            "Component",
            "Steps to Reproduce",
            "Expected Behavior",
            "Actual Behavior",
            "Workaround",
            "Root Cause",
            "Evidence",
        ]
        .map(String::from)
        .to_vec(),
        rows,
    })])
}

// ---------------------------------------------------------------------------
// test_plan
// ---------------------------------------------------------------------------

fn map_test_plan(data: &serde_json::Value) -> AppResult<Vec<ExportSection>> {
    let payload: TestPlanPayload = serde_json::from_value(data.clone())?;

    let risks: Vec<String> = payload
        .risks
        .iter()
        .map(|r| {
            if r.mitigation.is_empty() {
                r.description.clone()
            } else {
                format!("{} — Mitigation: {}", r.description, r.mitigation)
            }
        })
        .collect();

    let entries = vec![
        ("Summary".to_string(), clamp_cell(payload.summary)),
        ("Strategy".to_string(), clamp_cell(payload.strategy)),
        (
            "Objectives".to_string(),
            clamp_cell(numbered_lines(&payload.objectives)),
        ),
        (
            "Scope In".to_string(),
            clamp_cell(numbered_lines(&payload.scope_in)),
        ),
        (
            "Scope Out".to_string(),
            clamp_cell(numbered_lines(&payload.scope_out)),
        ),
        (
            "Environments".to_string(),
            clamp_cell(numbered_lines(&payload.environments)),
        ),
        ("Risks".to_string(), clamp_cell(numbered_lines(&risks))),
        (
            "Entry Criteria".to_string(),
            clamp_cell(numbered_lines(&payload.entry_criteria)),
        ),
        (
            "Exit Criteria".to_string(),
            clamp_cell(numbered_lines(&payload.exit_criteria)),
        ),
    ];

    Ok(vec![ExportSection::KeyValues(KeyValueSection {
        name: "Test Plan".into(),
        entries,
    })])
}

// ---------------------------------------------------------------------------
// context_md
// ---------------------------------------------------------------------------

fn map_context_md(data: &serde_json::Value) -> AppResult<Vec<ExportSection>> {
    let payload: ContextMdPayload = serde_json::from_value(data.clone())?;

    let modules: Vec<String> = payload
        .key_modules
        .iter()
        .map(|m| {
            if m.responsibility.is_empty() {
                m.name.clone()
            } else {
                format!("{} — {}", m.name, m.responsibility)
            }
        })
        .collect();
    let flows: Vec<String> = payload
        .data_flows
        .iter()
        .map(|f| format!("{} → {}: {}", f.producer, f.consumer, f.payload))
        .collect();

    let entries = vec![
        ("Summary".to_string(), clamp_cell(payload.summary)),
        (
            "Architecture Notes".to_string(),
            clamp_cell(payload.architecture_notes),
        ),
        (
            "Key Modules".to_string(),
            clamp_cell(numbered_lines(&modules)),
        ),
        ("Data Flows".to_string(), clamp_cell(numbered_lines(&flows))),
        (
            "Known Risks".to_string(),
            clamp_cell(numbered_lines(&payload.known_risks)),
        ),
    ];

    Ok(vec![ExportSection::KeyValues(KeyValueSection {
        name: "Context".into(),
        entries,
    })])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::artifact_repo::{ArtifactStatus, GenerationMetadata};

    fn artifact_with(artifact_type: ArtifactType, data: serde_json::Value) -> Artifact {
        Artifact {
            id: "a1".into(),
            project_id: "p1".into(),
            artifact_type,
            title: "Sample artifact".into(),
            content_md: "# md".into(),
            structured_data: data,
            generation_metadata: GenerationMetadata {
                provider: "ollama".into(),
                model: "qwen2.5-coder:7b".into(),
                prompt_version: "test".into(),
                input_tokens: 1,
                output_tokens: 1,
                started_at: "2026-06-07T00:00:00Z".into(),
                completed_at: "2026-06-07T00:00:01Z".into(),
            },
            status: ArtifactStatus::Draft,
            version: 1,
            parent_id: None,
            created_at: "2026-06-07T00:00:00Z".into(),
            updated_at: "2026-06-07T00:00:00Z".into(),
        }
    }

    #[test]
    fn null_payload_is_rejected() {
        let artifact = artifact_with(ArtifactType::TestCases, serde_json::Value::Null);
        let err = build_export_doc(&artifact).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn empty_object_payload_is_rejected() {
        let artifact = artifact_with(ArtifactType::BugReport, serde_json::json!({}));
        let err = build_export_doc(&artifact).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn empty_cases_array_still_yields_header_only_table() {
        let artifact = artifact_with(ArtifactType::TestCases, serde_json::json!({ "cases": [] }));
        let doc = build_export_doc(&artifact).expect("doc");
        assert_eq!(doc.sections.len(), 1);
        match &doc.sections[0] {
            ExportSection::Table(t) => {
                assert_eq!(t.name, "Test Cases");
                assert!(t.rows.is_empty());
                assert_eq!(t.columns.len(), 10);
            }
            ExportSection::KeyValues(_) => panic!("expected table"),
        }
    }

    #[test]
    fn test_cases_v2_snapshot() {
        let artifact = artifact_with(
            ArtifactType::TestCases,
            serde_json::json!({
                "cases": [{
                    "id": "TC-LOGIN-SUCCESS",
                    "title": "Valid login succeeds",
                    "type": "positive",
                    "priority": "p0",
                    "preconditions": ["User exists"],
                    "testData": "user@example.com / hunter2",
                    "steps": [
                        { "action": "Open login page", "expectedResult": "Form renders" },
                        { "action": "Submit valid creds", "expectedResult": "Redirect to home" }
                    ],
                    "postconditions": ["Session cookie set"],
                    "traceability": ["src/auth.ts#login"]
                }],
                "files": [
                    { "path": "src/auth.ts", "contents": "export function login() {}", "isTest": false },
                    { "path": "auth.test.ts", "contents": "import { it } from 'vitest'", "isTest": true }
                ]
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        insta::assert_yaml_snapshot!(doc);
    }

    #[test]
    fn test_cases_v1_legacy_steps_and_expected_result() {
        let artifact = artifact_with(
            ArtifactType::TestCases,
            serde_json::json!({
                "cases": [{
                    "id": "TC-ADD",
                    "title": "Adds two numbers",
                    "priority": "p1",
                    "steps": ["Call add(1, 2)", "Inspect return value"],
                    "expectedResult": "Returns 3",
                    "traceability": ["src/math.ts#add"]
                }]
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        match &doc.sections[0] {
            ExportSection::Table(t) => {
                let row = &t.rows[0];
                assert_eq!(row[6], "1. Call add(1, 2)\n2. Inspect return value");
                assert_eq!(row[7], "Returns 3");
                // v1 has no type / testData / postconditions.
                assert_eq!(row[2], "");
                assert_eq!(row[5], "");
            }
            ExportSection::KeyValues(_) => panic!("expected table"),
        }
    }

    #[test]
    fn defect_report_snapshot() {
        let artifact = artifact_with(
            ArtifactType::DefectReport,
            serde_json::json!({
                "findings": [{
                    "id": "DEF-NULL-POINTER",
                    "severity": "major",
                    "category": "null_safety",
                    "confidence": "high",
                    "location": {
                        "symbol": "save",
                        "start_line": 5,
                        "end_line": 15,
                        "file_hint": "src/store.ts"
                    },
                    "description": "save() dereferences a nullable handle.",
                    "impact": "Crash on empty store.",
                    "suggested_fix": "Guard with early return."
                }],
                "summary": "One null-safety finding."
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        insta::assert_yaml_snapshot!(doc);
    }

    #[test]
    fn bug_report_v2_snapshot() {
        let artifact = artifact_with(
            ArtifactType::BugReport,
            serde_json::json!({
                "bugs": [{
                    "id": "BUG-SESSION-LEAK",
                    "title": "Session handle leaks on logout",
                    "severity": "critical",
                    "priority": "p1",
                    "reproducibility": "always",
                    "environment": "Windows 11 / Node 20",
                    "component": "auth",
                    "stepsToReproduce": ["Log in", "Log out", "Inspect open handles"],
                    "expectedBehavior": "Handle closed",
                    "actualBehavior": "Handle stays open",
                    "workaround": "Restart the app",
                    "rootCause": {
                        "symbol": "logout",
                        "startLine": 10,
                        "endLine": 20,
                        "fileHint": "src/auth.ts",
                        "explanation": "Missing dispose() call."
                    },
                    "evidenceSnippet": "function logout() { /* no dispose */ }"
                }]
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        insta::assert_yaml_snapshot!(doc);
    }

    #[test]
    fn bug_report_v1_legacy_field_names_map() {
        let artifact = artifact_with(
            ArtifactType::BugReport,
            serde_json::json!({
                "bugs": [{
                    "id": "BUG-RACE",
                    "title": "Double write under load",
                    "severity": "major",
                    "environment": "linux",
                    "steps_to_reproduce": ["Run save twice"],
                    "expected_behavior": "Single write",
                    "actual_behavior": "Two writes",
                    "root_cause": {
                        "symbol": "save",
                        "start_line": 5,
                        "end_line": 15,
                        "file_hint": "src/store.ts",
                        "explanation": "No lock around write."
                    },
                    "evidence_snippet": "function save() {}"
                }]
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        match &doc.sections[0] {
            ExportSection::Table(t) => {
                let row = &t.rows[0];
                assert_eq!(row[7], "1. Run save twice");
                assert_eq!(row[8], "Single write");
                assert_eq!(row[9], "Two writes");
                assert!(row[11].contains("Symbol: save"));
                assert!(row[11].contains("Lines: 5–15"));
                // v2-only triage fields stay empty.
                assert_eq!(row[3], "");
                assert_eq!(row[4], "");
            }
            ExportSection::KeyValues(_) => panic!("expected table"),
        }
    }

    #[test]
    fn test_plan_snapshot() {
        let artifact = artifact_with(
            ArtifactType::TestPlan,
            serde_json::json!({
                "summary": "Plan for auth module.",
                "objectives": ["Cover login", "Cover logout"],
                "scopeIn": ["src/auth.ts"],
                "scopeOut": ["legacy/"],
                "strategy": "Unit-first with sandbox runs.",
                "environments": ["node 20"],
                "risks": [
                    { "description": "Flaky network mocks", "mitigation": "Use msw" },
                    { "description": "No CI minutes" }
                ],
                "entryCriteria": ["Build green"],
                "exitCriteria": ["All p0 pass"]
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        insta::assert_yaml_snapshot!(doc);
    }

    #[test]
    fn context_md_snapshot() {
        let artifact = artifact_with(
            ArtifactType::ContextMd,
            serde_json::json!({
                "summary": "Local-first testing IDE.",
                "architecture_notes": "* Tauri shell\n* Rust services",
                "key_modules": [
                    { "name": "generation_service", "responsibility": "LLM orchestration" }
                ],
                "data_flows": [
                    { "producer": "chunk_repo", "consumer": "generation_service", "payload": "chunks" }
                ],
                "known_risks": ["No Python chunker coverage"]
            }),
        );
        let doc = build_export_doc(&artifact).expect("doc");
        insta::assert_yaml_snapshot!(doc);
    }

    #[test]
    fn missing_optional_fields_never_panic() {
        // Each artifact type with a minimal one-key payload.
        let cases = vec![
            (ArtifactType::TestCases, serde_json::json!({ "cases": [{}] })),
            (
                ArtifactType::DefectReport,
                serde_json::json!({ "findings": [{}] }),
            ),
            (ArtifactType::BugReport, serde_json::json!({ "bugs": [{}] })),
            (
                ArtifactType::TestPlan,
                serde_json::json!({ "summary": "s" }),
            ),
            (
                ArtifactType::ContextMd,
                serde_json::json!({ "summary": "s" }),
            ),
        ];
        for (artifact_type, data) in cases {
            let artifact = artifact_with(artifact_type, data);
            build_export_doc(&artifact).expect("must map without panic");
        }
    }
}
