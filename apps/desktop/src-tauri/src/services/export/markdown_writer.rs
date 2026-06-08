//! Render an artifact's `structured_data` as real, human-readable
//! Markdown.
//!
//! This replaces the Phase-5 stopgap that dumped the JSON payload
//! into a fenced code block — "Export to Markdown" must produce a
//! document a human can read, not machine output. Values are emitted
//! verbatim (no cell clamping, unlike the spreadsheet IR) so nothing
//! is ever cut off.

use std::fmt::Write as _;

use serde_json::Value as JsonValue;

use crate::error::AppResult;
use crate::repositories::artifact_repo::ArtifactType;

use super::payload::{
    Bug, BugReportPayload, ContextMdPayload, DefectFinding, DefectReportPayload, RunnableFile,
    TestCase, TestCasesPayload, TestPlanPayload,
};

/// Render `structured_data` to Markdown for the given artifact type.
///
/// # Errors
///
/// - [`AppError::Serde`](crate::error::AppError::Serde) when the
///   payload cannot deserialize into the artifact type's expected
///   shape (every field is defaulted, so this only fires for
///   non-object payloads).
pub fn render_artifact_markdown(kind: ArtifactType, data: &JsonValue) -> AppResult<String> {
    let body = match kind {
        ArtifactType::TestCases => {
            render_test_cases(&serde_json::from_value(data.clone())?)
        }
        ArtifactType::DefectReport => {
            render_defect_report(&serde_json::from_value(data.clone())?)
        }
        ArtifactType::BugReport => render_bug_report(&serde_json::from_value(data.clone())?),
        ArtifactType::TestPlan => render_test_plan(&serde_json::from_value(data.clone())?),
        ArtifactType::ContextMd => render_context(&serde_json::from_value(data.clone())?),
    };

    let label = match kind {
        ArtifactType::ContextMd => "Project Context",
        ArtifactType::TestPlan => "Test Plan",
        ArtifactType::TestCases => "Test Cases",
        ArtifactType::DefectReport => "Defect Report",
        ArtifactType::BugReport => "Bug Report",
    };
    Ok(format!("# {label}\n\n{}\n", body.trim_end()))
}

// ---------------------------------------------------------------------------
// Shared building blocks
// ---------------------------------------------------------------------------

/// `## Heading` followed by prose. Skipped entirely when the value is
/// empty so the document never shows hollow sections.
fn prose_section(out: &mut String, heading: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let _ = writeln!(out, "## {heading}\n\n{}\n", value.trim());
}

/// `## Heading` followed by a bullet list. Skipped when empty.
fn list_section(out: &mut String, heading: &str, items: &[String]) {
    let rendered: Vec<&str> = items
        .iter()
        .map(|i| i.trim())
        .filter(|i| !i.is_empty())
        .collect();
    if rendered.is_empty() {
        return;
    }
    let _ = writeln!(out, "## {heading}\n");
    for item in rendered {
        let _ = writeln!(out, "- {item}");
    }
    let _ = writeln!(out);
}

/// `**Label:** value` line. Skipped when the value is empty.
fn labelled_line(out: &mut String, label: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let _ = writeln!(out, "**{label}:** {}", value.trim());
    let _ = writeln!(out);
}

/// `**Label:**` followed by a numbered list. Skipped when empty.
fn labelled_numbered_list(out: &mut String, label: &str, items: &[String]) {
    let rendered: Vec<&str> = items
        .iter()
        .map(|i| i.trim())
        .filter(|i| !i.is_empty())
        .collect();
    if rendered.is_empty() {
        return;
    }
    let _ = writeln!(out, "**{label}:**\n");
    for (idx, item) in rendered.iter().enumerate() {
        let _ = writeln!(out, "{}. {item}", idx + 1);
    }
    let _ = writeln!(out);
}

/// Fenced code block whose fence is always longer than any backtick
/// run inside `contents`, so an embedded triple-backtick sequence
/// cannot terminate the block early.
fn code_block(out: &mut String, language: &str, contents: &str) {
    let longest_backtick_run = contents
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest_backtick_run + 1).max(3));
    let _ = writeln!(out, "{fence}{language}");
    let _ = writeln!(out, "{}", contents.trim_end_matches('\n'));
    let _ = writeln!(out, "{fence}\n");
}

/// Best-effort fence language tag from a file extension.
fn fence_language(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("ts" | "tsx" | "mts" | "cts") => "ts",
        Some("js" | "jsx" | "mjs" | "cjs") => "js",
        Some("py") => "python",
        Some("json") => "json",
        _ => "",
    }
}

/// Heading text for an item that may be missing its id or title.
fn item_heading(id: &str, title: &str, fallback: &str, index: usize) -> String {
    match (id.trim(), title.trim()) {
        ("", "") => format!("{fallback} {}", index + 1),
        (id, "") => id.to_string(),
        ("", title) => title.to_string(),
        (id, title) => format!("{id} — {title}"),
    }
}

// ---------------------------------------------------------------------------
// test_cases
// ---------------------------------------------------------------------------

fn render_test_cases(payload: &TestCasesPayload) -> String {
    let mut out = String::new();
    for (idx, case) in payload.cases.iter().enumerate() {
        render_test_case(&mut out, case, idx);
    }
    if !payload.files.is_empty() {
        let _ = writeln!(out, "## Generated Files\n");
        for file in &payload.files {
            render_runnable_file(&mut out, file);
        }
    }
    out
}

fn render_test_case(out: &mut String, case: &TestCase, index: usize) {
    let _ = writeln!(
        out,
        "## {}\n",
        item_heading(&case.id, &case.title, "Test Case", index)
    );
    labelled_line(out, "Type", &case.case_type);
    labelled_line(out, "Priority", &case.priority);
    labelled_numbered_list(out, "Precondition", &case.preconditions);
    labelled_line(out, "Input Steps", &case.test_data);

    if !case.steps.is_empty() {
        let _ = writeln!(out, "**Steps to Reproduce:**\n");
        for (idx, step) in case.steps.iter().enumerate() {
            let expected = step.expected_result();
            if expected.is_empty() {
                let _ = writeln!(out, "{}. {}", idx + 1, step.action());
            } else {
                let _ = writeln!(
                    out,
                    "{}. {} — *Expected:* {}",
                    idx + 1,
                    step.action(),
                    expected
                );
            }
        }
        let _ = writeln!(out);
    }

    // v1 carries a single case-level expected result instead of
    // per-step ones.
    labelled_line(out, "Expected Output", &case.expected_result);
    labelled_numbered_list(out, "Postconditions", &case.postconditions);
    if !case.traceability.is_empty() {
        labelled_line(out, "Traceability", &case.traceability.join(", "));
    }
}

fn render_runnable_file(out: &mut String, file: &RunnableFile) {
    let role = if file.is_test { "test" } else { "source" };
    let _ = writeln!(out, "### `{}` ({role})\n", file.path);
    code_block(out, fence_language(&file.path), &file.contents);
}

// ---------------------------------------------------------------------------
// defect_report
// ---------------------------------------------------------------------------

fn render_defect_report(payload: &DefectReportPayload) -> String {
    let mut out = String::new();
    prose_section(&mut out, "Summary", &payload.summary);
    for (idx, finding) in payload.findings.iter().enumerate() {
        render_defect_finding(&mut out, finding, idx);
    }
    out
}

fn render_defect_finding(out: &mut String, finding: &DefectFinding, index: usize) {
    let _ = writeln!(
        out,
        "## {}\n",
        item_heading(&finding.id, "", "Finding", index)
    );
    labelled_line(out, "Severity", &finding.severity);
    labelled_line(out, "Category", &finding.category);
    labelled_line(out, "Confidence", &finding.confidence);
    labelled_line(out, "Location", &finding.location.flatten());
    labelled_line(out, "Description", &finding.description);
    labelled_line(out, "Impact", &finding.impact);
    labelled_line(out, "Suggested Fix", &finding.suggested_fix);
}

// ---------------------------------------------------------------------------
// bug_report
// ---------------------------------------------------------------------------

fn render_bug_report(payload: &BugReportPayload) -> String {
    let mut out = String::new();
    for (idx, bug) in payload.bugs.iter().enumerate() {
        render_bug(&mut out, bug, idx);
    }
    out
}

fn render_bug(out: &mut String, bug: &Bug, index: usize) {
    let _ = writeln!(out, "## {}\n", item_heading(&bug.id, &bug.title, "Bug", index));
    labelled_line(out, "Severity", &bug.severity);
    labelled_line(out, "Priority", &bug.priority);
    labelled_line(out, "Reproducibility", &bug.reproducibility);
    labelled_line(out, "Environment", &bug.environment);
    labelled_line(out, "Component", &bug.component);
    labelled_numbered_list(out, "Steps to Reproduce", &bug.steps_to_reproduce);
    labelled_line(out, "Expected Behavior", &bug.expected_behavior);
    labelled_line(out, "Actual Behavior", &bug.actual_behavior);
    labelled_line(out, "Workaround", &bug.workaround);

    let root_cause = bug.root_cause.flatten();
    if !root_cause.is_empty() {
        let _ = writeln!(out, "**Root Cause:**\n");
        for line in root_cause.lines() {
            let _ = writeln!(out, "- {line}");
        }
        let _ = writeln!(out);
    }

    if !bug.evidence_snippet.trim().is_empty() {
        let _ = writeln!(out, "**Evidence:**\n");
        code_block(out, "", &bug.evidence_snippet);
    }
}

// ---------------------------------------------------------------------------
// test_plan
// ---------------------------------------------------------------------------

fn render_test_plan(payload: &TestPlanPayload) -> String {
    let mut out = String::new();
    prose_section(&mut out, "Summary", &payload.summary);
    list_section(&mut out, "Objectives", &payload.objectives);
    list_section(&mut out, "In Scope", &payload.scope_in);
    list_section(&mut out, "Out of Scope", &payload.scope_out);
    prose_section(&mut out, "Strategy", &payload.strategy);
    list_section(&mut out, "Environments", &payload.environments);

    let risks: Vec<String> = payload
        .risks
        .iter()
        .map(|r| {
            if r.mitigation.trim().is_empty() {
                r.description.clone()
            } else {
                format!("{} — **Mitigation:** {}", r.description, r.mitigation)
            }
        })
        .collect();
    list_section(&mut out, "Risks", &risks);
    list_section(&mut out, "Entry Criteria", &payload.entry_criteria);
    list_section(&mut out, "Exit Criteria", &payload.exit_criteria);
    out
}

// ---------------------------------------------------------------------------
// context_md
// ---------------------------------------------------------------------------

fn render_context(payload: &ContextMdPayload) -> String {
    let mut out = String::new();
    prose_section(&mut out, "Summary", &payload.summary);
    prose_section(&mut out, "Architecture Notes", &payload.architecture_notes);

    let modules: Vec<String> = payload
        .key_modules
        .iter()
        .map(|m| {
            if m.responsibility.trim().is_empty() {
                format!("**{}**", m.name)
            } else {
                format!("**{}** — {}", m.name, m.responsibility)
            }
        })
        .collect();
    list_section(&mut out, "Key Modules", &modules);

    let flows: Vec<String> = payload
        .data_flows
        .iter()
        .map(|f| format!("{} → {}: {}", f.producer, f.consumer, f.payload))
        .collect();
    list_section(&mut out, "Data Flows", &flows);
    list_section(&mut out, "Known Risks", &payload.known_risks);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_renders_headed_sections_not_json() {
        let data = serde_json::json!({
            "summary": "Validate the auth subsystem.",
            "objectives": ["Verify login", "Verify logout"],
            "scopeIn": ["auth module"],
            "scopeOut": ["legacy/"],
            "strategy": "Risk-based API checks.",
            "environments": ["node 20"],
            "risks": [
                { "description": "Token reuse", "mitigation": "Verify revocation" },
                { "description": "No CI minutes" }
            ],
            "entryCriteria": ["Build green"],
            "exitCriteria": ["All p0 pass"]
        });
        let md = render_artifact_markdown(ArtifactType::TestPlan, &data).expect("render");

        assert!(md.starts_with("# Test Plan\n"));
        assert!(md.contains("## Summary\n\nValidate the auth subsystem."));
        assert!(md.contains("## Objectives\n\n- Verify login\n- Verify logout"));
        assert!(md.contains("## In Scope\n\n- auth module"));
        assert!(md.contains("## Out of Scope\n\n- legacy/"));
        assert!(md.contains("- Token reuse — **Mitigation:** Verify revocation"));
        assert!(md.contains("- No CI minutes"));
        assert!(!md.contains("```json"));
        assert!(!md.contains("\"summary\""));
    }

    #[test]
    fn context_renders_modules_flows_and_risks() {
        let data = serde_json::json!({
            "summary": "Local-first testing IDE.",
            "architecture_notes": "Tauri shell over a Rust core.",
            "key_modules": [
                { "name": "generation_service", "responsibility": "LLM orchestration" },
                { "name": "chunk_repo" }
            ],
            "data_flows": [
                { "producer": "chunk_repo", "consumer": "generation_service", "payload": "chunks" }
            ],
            "known_risks": ["No Python chunker coverage"]
        });
        let md = render_artifact_markdown(ArtifactType::ContextMd, &data).expect("render");

        assert!(md.starts_with("# Project Context\n"));
        assert!(md.contains("**generation_service** — LLM orchestration"));
        assert!(md.contains("- **chunk_repo**"));
        assert!(md.contains("chunk_repo → generation_service: chunks"));
        assert!(md.contains("## Known Risks\n\n- No Python chunker coverage"));
        assert!(!md.contains("```json"));
    }

    #[test]
    fn test_cases_v2_renders_steps_with_expected_results() {
        let data = serde_json::json!({
            "cases": [{
                "id": "TC-LOGIN-1",
                "title": "Valid login succeeds",
                "type": "positive",
                "priority": "p0",
                "preconditions": ["User exists"],
                "testData": "user@example.com / hunter2",
                "steps": [
                    { "action": "Open login page", "expectedResult": "Form renders" },
                    { "action": "Submit valid creds", "expectedResult": "Redirect home" }
                ],
                "postconditions": ["Session cookie set"],
                "traceability": ["src/auth.ts#login"]
            }],
            "files": [
                { "path": "auth.test.ts", "contents": "import { it } from 'vitest';", "isTest": true }
            ]
        });
        let md = render_artifact_markdown(ArtifactType::TestCases, &data).expect("render");

        assert!(md.contains("## TC-LOGIN-1 — Valid login succeeds"));
        assert!(md.contains("**Priority:** p0"));
        assert!(md.contains("1. Open login page — *Expected:* Form renders"));
        assert!(md.contains("2. Submit valid creds — *Expected:* Redirect home"));
        assert!(md.contains("## Generated Files"));
        assert!(md.contains("### `auth.test.ts` (test)"));
        assert!(md.contains("```ts\nimport { it } from 'vitest';\n```"));
    }

    #[test]
    fn test_cases_v1_plain_steps_and_case_level_expected_result() {
        let data = serde_json::json!({
            "cases": [{
                "id": "TC-ADD",
                "title": "Adds two numbers",
                "steps": ["Call add(1, 2)", "Inspect return value"],
                "expectedResult": "Returns 3"
            }]
        });
        let md = render_artifact_markdown(ArtifactType::TestCases, &data).expect("render");

        assert!(md.contains("1. Call add(1, 2)\n2. Inspect return value"));
        assert!(md.contains("**Expected Output:** Returns 3"));
    }

    #[test]
    fn bug_report_renders_triage_fields_and_evidence_fence() {
        let data = serde_json::json!({
            "bugs": [{
                "id": "BUG-1",
                "title": "Session leak",
                "severity": "critical",
                "priority": "p1",
                "stepsToReproduce": ["Log in", "Log out"],
                "expectedBehavior": "Handle closed",
                "actualBehavior": "Handle stays open",
                "rootCause": {
                    "symbol": "logout",
                    "startLine": 10,
                    "endLine": 20,
                    "fileHint": "src/auth.ts",
                    "explanation": "Missing dispose() call."
                },
                "evidenceSnippet": "function logout() {}"
            }]
        });
        let md = render_artifact_markdown(ArtifactType::BugReport, &data).expect("render");

        assert!(md.contains("## BUG-1 — Session leak"));
        assert!(md.contains("**Severity:** critical"));
        assert!(md.contains("1. Log in\n2. Log out"));
        assert!(md.contains("- Symbol: logout"));
        assert!(md.contains("- Explanation: Missing dispose() call."));
        assert!(md.contains("```\nfunction logout() {}\n```"));
    }

    #[test]
    fn defect_report_renders_summary_and_findings() {
        let data = serde_json::json!({
            "summary": "One null-safety finding.",
            "findings": [{
                "id": "DEF-1",
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
            }]
        });
        let md = render_artifact_markdown(ArtifactType::DefectReport, &data).expect("render");

        assert!(md.contains("## Summary\n\nOne null-safety finding."));
        assert!(md.contains("## DEF-1"));
        assert!(md.contains("**Location:** src/store.ts · `save` · lines 5–15"));
        assert!(md.contains("**Suggested Fix:** Guard with early return."));
    }

    #[test]
    fn empty_sections_are_omitted() {
        let data = serde_json::json!({ "summary": "Just a summary." });
        let md = render_artifact_markdown(ArtifactType::TestPlan, &data).expect("render");

        assert!(md.contains("## Summary"));
        assert!(!md.contains("## Objectives"));
        assert!(!md.contains("## Risks"));
    }

    #[test]
    fn long_values_are_never_truncated() {
        let long = "x".repeat(50_000);
        let data = serde_json::json!({ "summary": long });
        let md = render_artifact_markdown(ArtifactType::ContextMd, &data).expect("render");

        assert!(md.contains(&"x".repeat(50_000)));
        assert!(!md.contains("truncated"));
    }

    #[test]
    fn code_fence_grows_past_embedded_backtick_runs() {
        let data = serde_json::json!({
            "cases": [],
            "files": [{
                "path": "weird.md",
                "contents": "before\n````\ninner\n````\nafter",
                "isTest": false
            }]
        });
        let md = render_artifact_markdown(ArtifactType::TestCases, &data).expect("render");

        // The fence must be at least 5 backticks so the embedded
        // 4-backtick run cannot close it.
        assert!(md.contains("`````"));
    }

    #[test]
    fn non_object_payload_errors() {
        let data = serde_json::json!("not an object");
        assert!(render_artifact_markdown(ArtifactType::TestPlan, &data).is_err());
    }

    #[test]
    fn missing_ids_fall_back_to_indexed_headings() {
        let data = serde_json::json!({ "bugs": [{}, {}] });
        let md = render_artifact_markdown(ArtifactType::BugReport, &data).expect("render");
        assert!(md.contains("## Bug 1"));
        assert!(md.contains("## Bug 2"));
    }
}
