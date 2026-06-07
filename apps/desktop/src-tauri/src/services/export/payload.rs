//! Deserialized `structured_data` payload shapes, shared by every
//! export renderer (IR mappers in `mappers.rs`, the markdown writer
//! in `markdown_writer.rs`).
//!
//! Payload shapes mirror the prompt tool schemas (`prompts/*_v1.rs` /
//! `*_v2.rs`). Every field uses `#[serde(default)]` plus aliases for
//! legacy spellings (v1 `snake_case` vs v2 `camelCase`) so old DB rows
//! and partially-populated payloads deserialize without panicking.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// test_cases (v1 + v2)
// ---------------------------------------------------------------------------

/// One test step. v2 uses separated `{ action, expectedResult }`
/// objects; v1 rows carry plain strings. `untagged` lets one payload
/// mix both (defensive — should not happen in practice).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum TestStep {
    Separated {
        #[serde(default)]
        action: String,
        #[serde(default, alias = "expected_result")]
        #[serde(rename = "expectedResult")]
        expected_result: String,
    },
    Plain(String),
}

impl TestStep {
    /// The action half of the step (the whole string for v1 steps).
    pub(crate) fn action(&self) -> &str {
        match self {
            Self::Separated { action, .. } => action,
            Self::Plain(text) => text,
        }
    }

    /// The per-step expected result (empty for v1 steps).
    pub(crate) fn expected_result(&self) -> &str {
        match self {
            Self::Separated {
                expected_result, ..
            } => expected_result,
            Self::Plain(_) => "",
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TestCase {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) title: String,
    /// v2 only — positive / negative / boundary / error / security.
    #[serde(default, rename = "type")]
    pub(crate) case_type: String,
    #[serde(default)]
    pub(crate) priority: String,
    #[serde(default)]
    pub(crate) preconditions: Vec<String>,
    /// v2 only.
    #[serde(default, rename = "testData", alias = "test_data")]
    pub(crate) test_data: String,
    #[serde(default)]
    pub(crate) steps: Vec<TestStep>,
    /// v1 only — single case-level expected result.
    #[serde(default, rename = "expectedResult", alias = "expected_result")]
    pub(crate) expected_result: String,
    /// v2 only.
    #[serde(default)]
    pub(crate) postconditions: Vec<String>,
    #[serde(default)]
    pub(crate) traceability: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RunnableFile {
    #[serde(default)]
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) contents: String,
    #[serde(default, rename = "isTest", alias = "is_test")]
    pub(crate) is_test: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TestCasesPayload {
    #[serde(default)]
    pub(crate) cases: Vec<TestCase>,
    #[serde(default)]
    pub(crate) files: Vec<RunnableFile>,
}

// ---------------------------------------------------------------------------
// defect_report
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DefectLocation {
    #[serde(default)]
    pub(crate) symbol: String,
    #[serde(default, alias = "startLine")]
    pub(crate) start_line: u64,
    #[serde(default, alias = "endLine")]
    pub(crate) end_line: u64,
    #[serde(default, alias = "fileHint")]
    pub(crate) file_hint: String,
}

impl DefectLocation {
    /// Single-line `file · symbol · lines` rendering shared by every
    /// export format.
    pub(crate) fn flatten(&self) -> String {
        let mut parts = Vec::new();
        if !self.file_hint.is_empty() {
            parts.push(self.file_hint.clone());
        }
        if !self.symbol.is_empty() {
            parts.push(format!("`{}`", self.symbol));
        }
        if self.start_line > 0 {
            parts.push(format!("lines {}–{}", self.start_line, self.end_line));
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DefectFinding {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) severity: String,
    #[serde(default)]
    pub(crate) category: String,
    #[serde(default)]
    pub(crate) confidence: String,
    #[serde(default)]
    pub(crate) location: DefectLocation,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) impact: String,
    #[serde(default, rename = "suggested_fix", alias = "suggestedFix")]
    pub(crate) suggested_fix: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DefectReportPayload {
    #[serde(default)]
    pub(crate) findings: Vec<DefectFinding>,
    #[serde(default)]
    pub(crate) summary: String,
}

// ---------------------------------------------------------------------------
// bug_report (v1 + v2)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub(crate) struct BugRootCause {
    #[serde(default)]
    pub(crate) symbol: String,
    #[serde(default, rename = "startLine", alias = "start_line")]
    pub(crate) start_line: u64,
    #[serde(default, rename = "endLine", alias = "end_line")]
    pub(crate) end_line: u64,
    #[serde(default, rename = "fileHint", alias = "file_hint")]
    pub(crate) file_hint: String,
    #[serde(default)]
    pub(crate) explanation: String,
}

impl BugRootCause {
    /// Multi-line `Label: value` rendering shared by every export
    /// format.
    pub(crate) fn flatten(&self) -> String {
        let mut lines = Vec::new();
        if !self.symbol.is_empty() {
            lines.push(format!("Symbol: {}", self.symbol));
        }
        if !self.file_hint.is_empty() {
            lines.push(format!("File: {}", self.file_hint));
        }
        if self.start_line > 0 {
            lines.push(format!("Lines: {}–{}", self.start_line, self.end_line));
        }
        if !self.explanation.is_empty() {
            lines.push(format!("Explanation: {}", self.explanation));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Bug {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) severity: String,
    /// v2 only — fix urgency, split from severity.
    #[serde(default)]
    pub(crate) priority: String,
    /// v2 only.
    #[serde(default)]
    pub(crate) reproducibility: String,
    #[serde(default)]
    pub(crate) environment: String,
    /// v2 only.
    #[serde(default)]
    pub(crate) component: String,
    #[serde(default, rename = "stepsToReproduce", alias = "steps_to_reproduce")]
    pub(crate) steps_to_reproduce: Vec<String>,
    #[serde(default, rename = "expectedBehavior", alias = "expected_behavior")]
    pub(crate) expected_behavior: String,
    #[serde(default, rename = "actualBehavior", alias = "actual_behavior")]
    pub(crate) actual_behavior: String,
    /// v2 only.
    #[serde(default)]
    pub(crate) workaround: String,
    #[serde(default, rename = "rootCause", alias = "root_cause")]
    pub(crate) root_cause: BugRootCause,
    #[serde(default, rename = "evidenceSnippet", alias = "evidence_snippet")]
    pub(crate) evidence_snippet: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BugReportPayload {
    #[serde(default)]
    pub(crate) bugs: Vec<Bug>,
}

// ---------------------------------------------------------------------------
// test_plan
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TestPlanRisk {
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) mitigation: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TestPlanPayload {
    #[serde(default)]
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) objectives: Vec<String>,
    #[serde(default, rename = "scopeIn", alias = "scope_in")]
    pub(crate) scope_in: Vec<String>,
    #[serde(default, rename = "scopeOut", alias = "scope_out")]
    pub(crate) scope_out: Vec<String>,
    #[serde(default)]
    pub(crate) strategy: String,
    #[serde(default)]
    pub(crate) environments: Vec<String>,
    #[serde(default)]
    pub(crate) risks: Vec<TestPlanRisk>,
    #[serde(default, rename = "entryCriteria", alias = "entry_criteria")]
    pub(crate) entry_criteria: Vec<String>,
    #[serde(default, rename = "exitCriteria", alias = "exit_criteria")]
    pub(crate) exit_criteria: Vec<String>,
}

// ---------------------------------------------------------------------------
// context_md
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub(crate) struct KeyModule {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) responsibility: String,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DataFlow {
    #[serde(default)]
    pub(crate) producer: String,
    #[serde(default)]
    pub(crate) consumer: String,
    #[serde(default)]
    pub(crate) payload: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContextMdPayload {
    #[serde(default)]
    pub(crate) summary: String,
    #[serde(default, rename = "architecture_notes", alias = "architectureNotes")]
    pub(crate) architecture_notes: String,
    #[serde(default, rename = "key_modules", alias = "keyModules")]
    pub(crate) key_modules: Vec<KeyModule>,
    #[serde(default, rename = "data_flows", alias = "dataFlows")]
    pub(crate) data_flows: Vec<DataFlow>,
    #[serde(default, rename = "known_risks", alias = "knownRisks")]
    pub(crate) known_risks: Vec<String>,
}
