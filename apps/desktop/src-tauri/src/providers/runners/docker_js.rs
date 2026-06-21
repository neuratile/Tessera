//! JS/TS sandboxed test runner backed by local Docker (plan §7).
//!
//! Builds a throwaway workspace from a [`RunInput`], runs the suite inside
//! a hardened, network-isolated container, then parses the vitest results
//! and istanbul coverage the container writes back into the workspace.
//!
//! All Docker plumbing — the canonical hardening flag set, the
//! timeout / cancel → `docker kill` orchestration, the RAII workspace
//! cleanup, and the output truncation caps — lives in
//! [`docker_harness`](super::docker_harness) and is shared with every
//! other Docker runner (`docker_py`), so the sandbox cannot drift weaker
//! for one language. This file owns only what is JS-specific: the image
//! name, the vitest invocation + config, and the vitest/istanbul parsers.

use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

use super::docker_harness::{
    self, derive_status, f64_to_u32, truncate, truncate_to, ContainerOutput, WorkspaceGuard,
    MAX_FAILURE_MSG_BYTES, MAX_TEST_NAME_BYTES,
};
use super::{
    CancelToken, CoverageLine, RunInput, RunnerError, RunnerLanguage, RunnerOutput, TestResult,
    TestRunner, TestStatus,
};

/// Pre-built runner image (plan §7). Built locally from
/// `docker/Dockerfile.runner-js`, never pulled from a registry (local-first
/// guarantee — see ADR-0004 and `docker_harness::ensure_runner_image`).
/// Ships `vitest` + the istanbul coverage provider pre-installed so a run
/// needs no `npm install` (fast, deterministic, offline).
pub const RUNNER_IMAGE: &str = "tessera-runner-js";

/// Dockerfile (under `apps/desktop/src-tauri/docker/`) the image is built
/// from; surfaced in the `ImageMissing` build hint.
const RUNNER_DOCKERFILE: &str = "Dockerfile.runner-js";

/// Filenames the in-container command writes back into the workspace.
const RESULTS_FILE: &str = "results.json";
const COVERAGE_FILE: &str = "coverage/coverage-final.json";

/// Docker-backed JS/TS [`TestRunner`].
#[derive(Debug, Clone, Default)]
pub struct DockerJsRunner {
    /// Root for throwaway workspaces. Defaults to the OS temp dir; tests
    /// can point it elsewhere.
    workspace_root: Option<std::path::PathBuf>,
}

impl DockerJsRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn workspace_root(&self) -> std::path::PathBuf {
        self.workspace_root
            .clone()
            .unwrap_or_else(std::env::temp_dir)
    }
}

#[async_trait]
impl TestRunner for DockerJsRunner {
    fn name(&self) -> &'static str {
        "docker-js"
    }

    async fn run(
        &self,
        input: RunInput,
        cancel: CancelToken,
    ) -> Result<RunnerOutput, RunnerError> {
        input.validate()?;
        docker_harness::ensure_docker_available().await?;
        docker_harness::ensure_runner_image(RUNNER_IMAGE, RUNNER_DOCKERFILE).await?;

        // Workspace is removed when `guard` drops — covers the happy path,
        // every `?` early-return, and a panic (§10: always cleaned up).
        let guard = WorkspaceGuard::create(&self.workspace_root())?;
        tracing::debug!(files = input.files.len(), "materializing workspace");
        materialize_workspace(guard.path(), &input)?;

        tracing::debug!(language = ?input.language, "starting container");
        let output: ContainerOutput =
            docker_harness::run_container(guard.path(), RUNNER_IMAGE, IN_CONTAINER_CMD, &input, &cancel)
                .await?;
        let stdout = truncate(&output.stdout);
        let stderr = truncate(&output.stderr);

        let results_path = guard.path().join(RESULTS_FILE);
        let coverage_path = guard.path().join(COVERAGE_FILE);

        // vitest exits non-zero when assertions fail — that is a normal
        // `Failed` run, not a process error. Only treat a missing results
        // file as a genuine failure.
        let results_json = std::fs::read_to_string(&results_path).map_err(|e| {
            RunnerError::Process(format!(
                "vitest produced no results file ({e}); container exit {}, stderr: {}",
                output.exit_code, stderr
            ))
        })?;

        let tests = parse_vitest_results(&results_json)?;
        let coverage = std::fs::read_to_string(&coverage_path)
            .ok()
            .map(|json| parse_istanbul_coverage(&json))
            .transpose()?
            .unwrap_or_default();

        tracing::debug!(
            tests = tests.len(),
            coverage_lines = coverage.len(),
            "parsed runner output"
        );

        let status = derive_status(&tests);

        Ok(RunnerOutput {
            status,
            tests,
            coverage,
            stdout,
            stderr,
        })
    }
}

/// Paths the runner writes and then reads back after the container exits
/// (`RESULTS_FILE`, the `coverage/` report dir). A crafted artifact must not
/// be allowed to pre-seed these — see `docker_harness::materialize_files`.
fn is_reserved_output_path(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    normalized == RESULTS_FILE || normalized == "coverage" || normalized.starts_with("coverage/")
}

/// Write the source/test files plus a minimal `package.json` and vitest
/// config into the workspace.
fn materialize_workspace(root: &Path, input: &RunInput) -> Result<(), RunnerError> {
    docker_harness::materialize_files(root, input, is_reserved_output_path)?;

    std::fs::write(root.join("package.json"), PACKAGE_JSON)
        .map_err(|e| RunnerError::Workspace(format!("write package.json: {e}")))?;

    let config_name = match input.language {
        RunnerLanguage::TypeScript => "vitest.config.ts",
        // Python never reaches this runner (factory routes it to
        // `docker_py`), but the match must stay exhaustive.
        RunnerLanguage::JavaScript | RunnerLanguage::Python => "vitest.config.js",
    };
    std::fs::write(root.join(config_name), VITEST_CONFIG)
        .map_err(|e| RunnerError::Workspace(format!("write {config_name}: {e}")))?;

    Ok(())
}

/// Command run inside the container. Emits a vitest JSON report and an
/// istanbul `coverage-final.json`, both into the mounted workspace.
// `--no-file-parallelism` runs test files in a single worker instead of
// spawning one per host core (paired with the GOMAXPROCS cap in
// `docker_harness`, this keeps the container's thread/process count bounded
// — see the EAGAIN "failed to create new OS thread" failure mode).
const IN_CONTAINER_CMD: &str = "vitest run --coverage --no-file-parallelism \
     --reporter=json --outputFile=results.json \
     --coverage.reporter=json --coverage.reportsDirectory=coverage";

/// Minimal manifest written into the workspace. The runner image already
/// has `vitest` + coverage tooling installed, so no install runs.
const PACKAGE_JSON: &str = r#"{
  "name": "tessera-sandbox-run",
  "private": true,
  "type": "module"
}
"#;

/// Vitest config enabling JSON coverage. `includeTaskLocation` makes the
/// JSON reporter emit each assertion's source `location` (line/column) so
/// the editor can anchor a failing test to its line (§8).
const VITEST_CONFIG: &str = r"import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    includeTaskLocation: true,
    coverage: {
      provider: 'istanbul',
      enabled: true,
      all: true,
    },
  },
});
";

// ---------------------------------------------------------------------------
// Parsers — pure functions, unit-tested below without Docker.
// ---------------------------------------------------------------------------

/// Subset of the vitest `--reporter=json` shape we consume.
#[derive(Debug, Deserialize)]
struct VitestReport {
    #[serde(default)]
    #[serde(rename = "testResults")]
    test_results: Vec<VitestFile>,
}

#[derive(Debug, Deserialize)]
struct VitestFile {
    #[serde(default)]
    #[serde(rename = "assertionResults")]
    assertion_results: Vec<VitestAssertion>,
}

#[derive(Debug, Deserialize)]
struct VitestAssertion {
    #[serde(default)]
    title: String,
    #[serde(rename = "fullName")]
    full_name: Option<String>,
    status: String,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    #[serde(rename = "failureMessages")]
    failure_messages: Vec<String>,
    /// Source location of the test, emitted by the reporter only when
    /// `includeTaskLocation` is on (see [`VITEST_CONFIG`]). Absent on older
    /// reporters or skipped-without-location cases → `source_line` stays
    /// `None`.
    #[serde(default)]
    location: Option<VitestLocation>,
}

/// `{ line, column }` of a test in its spec file. Only `line` is consumed.
#[derive(Debug, Deserialize)]
struct VitestLocation {
    #[serde(default)]
    line: u32,
}

/// Parse the vitest JSON reporter output into [`TestResult`]s.
///
/// # Errors
///
/// [`RunnerError::Parse`] when the JSON is malformed.
pub fn parse_vitest_results(json: &str) -> Result<Vec<TestResult>, RunnerError> {
    let report: VitestReport = serde_json::from_str(json)
        .map_err(|e| RunnerError::Parse(format!("vitest report: {e}")))?;

    let mut tests = Vec::new();
    for file in report.test_results {
        for assertion in file.assertion_results {
            let status = match assertion.status.as_str() {
                "passed" => TestStatus::Passed,
                "failed" => TestStatus::Failed,
                // "skipped" | "pending" | "todo" | "disabled"
                _ => TestStatus::Skipped,
            };
            let raw_name = assertion
                .full_name
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(assertion.title);
            let name = truncate_to(&raw_name, MAX_TEST_NAME_BYTES);
            let failure_message = if assertion.failure_messages.is_empty() {
                None
            } else {
                Some(truncate_to(
                    &assertion.failure_messages.join("\n"),
                    MAX_FAILURE_MSG_BYTES,
                ))
            };
            let duration_ms = assertion
                .duration
                .filter(|d| d.is_finite() && *d >= 0.0)
                .map_or(0, f64_to_u32);
            // 1-based line of the test in its spec; drop a 0/absent location
            // so a missing anchor stays `None` rather than line 0 (§8).
            let source_line = assertion.location.map(|loc| loc.line).filter(|line| *line > 0);
            tests.push(TestResult {
                name,
                status,
                duration_ms,
                failure_message,
                source_line,
            });
        }
    }
    Ok(tests)
}

/// Flatten an istanbul `coverage-final.json` into per-line hit counts.
///
/// Each file carries a `statementMap` (id → location) and `s` (id → hit
/// count). A single source line often holds several statements, so we
/// aggregate by `(file, line)`: the line's hit count is the **max** over
/// its statements (the number of times the line executed), and a line is
/// reported uncovered (`hits == 0`) only when every statement on it has
/// zero hits. Output is sorted by file then line (`BTreeMap` order),
/// matching the read-back ordering in `test_run_repo::fetch_run`.
///
/// # Errors
///
/// [`RunnerError::Parse`] when the JSON is malformed.
pub fn parse_istanbul_coverage(json: &str) -> Result<Vec<CoverageLine>, RunnerError> {
    let root: std::collections::BTreeMap<String, IstanbulFile> = serde_json::from_str(json)
        .map_err(|e| RunnerError::Parse(format!("istanbul coverage: {e}")))?;

    let mut by_line: std::collections::BTreeMap<(String, u32), u32> =
        std::collections::BTreeMap::new();
    for (file_path, file) in root {
        for (id, location) in file.statement_map {
            let line = location.start.line;
            if line == 0 {
                continue;
            }
            let hits = file.s.get(&id).copied().unwrap_or(0);
            by_line
                .entry((file_path.clone(), line))
                .and_modify(|h| *h = (*h).max(hits))
                .or_insert(hits);
        }
    }

    Ok(by_line
        .into_iter()
        .map(|((file_path, line), hits)| CoverageLine { file_path, line, hits })
        .collect())
}

#[derive(Debug, Deserialize)]
struct IstanbulFile {
    #[serde(rename = "statementMap")]
    statement_map: std::collections::BTreeMap<String, IstanbulStatement>,
    s: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Deserialize)]
struct IstanbulStatement {
    start: IstanbulLoc,
}

#[derive(Debug, Deserialize)]
struct IstanbulLoc {
    #[serde(default)]
    line: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::runners::{ResourceLimits, RunStatus, WorkspaceFile};

    #[test]
    fn parse_vitest_results_maps_pass_fail_skip() {
        let json = r#"{
            "testResults": [
                {
                    "assertionResults": [
                        {"title": "adds", "fullName": "math > adds", "status": "passed", "duration": 12.0, "failureMessages": []},
                        {"title": "rejects", "fullName": "math > rejects", "status": "failed", "duration": 4.5, "failureMessages": ["expected 2 to equal 3"]},
                        {"title": "todo later", "status": "skipped", "failureMessages": []}
                    ]
                }
            ]
        }"#;
        let tests = parse_vitest_results(json).expect("parse");
        assert_eq!(tests.len(), 3);

        assert_eq!(tests[0].name, "math > adds");
        assert_eq!(tests[0].status, TestStatus::Passed);
        assert_eq!(tests[0].duration_ms, 12);
        assert!(tests[0].failure_message.is_none());

        assert_eq!(tests[1].status, TestStatus::Failed);
        assert_eq!(tests[1].duration_ms, 4); // truncated toward zero
        assert_eq!(tests[1].failure_message.as_deref(), Some("expected 2 to equal 3"));

        // No fullName → falls back to title.
        assert_eq!(tests[2].name, "todo later");
        assert_eq!(tests[2].status, TestStatus::Skipped);

        // No `location` in this payload → source line stays None.
        assert!(tests.iter().all(|t| t.source_line.is_none()));
    }

    #[test]
    fn parse_vitest_results_fixture_maps_locations_and_statuses() {
        let json = include_str!("fixtures/vitest-report.json");
        let tests = parse_vitest_results(json).expect("parse fixture");
        assert_eq!(tests.len(), 4);

        assert_eq!(tests[0].name, "add > adds two positive numbers");
        assert_eq!(tests[0].status, TestStatus::Passed);
        assert_eq!(tests[0].duration_ms, 3);
        assert_eq!(tests[0].source_line, Some(5)); // anchored from `location`

        assert_eq!(tests[1].status, TestStatus::Passed);
        assert_eq!(tests[1].source_line, Some(9));

        assert_eq!(tests[2].status, TestStatus::Failed);
        assert_eq!(tests[2].source_line, Some(13));
        assert!(
            tests[2]
                .failure_message
                .as_deref()
                .expect("failure message")
                .contains("expected NaN")
        );

        assert_eq!(tests[3].status, TestStatus::Skipped);
        assert_eq!(tests[3].source_line, Some(18));
    }

    #[test]
    fn parse_istanbul_coverage_fixture_dedupes_lines_by_max_hits() {
        let json = include_str!("fixtures/istanbul-coverage.json");
        let coverage = parse_istanbul_coverage(json).expect("parse fixture");

        // 4 statements across 3 distinct lines → 3 deduped entries, already
        // sorted by file then line (BTreeMap order).
        assert_eq!(coverage.len(), 3);
        assert!(coverage.iter().all(|c| c.file_path == "/work/src/add.ts"));

        assert_eq!(coverage[0].line, 1);
        assert_eq!(coverage[0].hits, 5);
        // Line 2 carries two statements (hits 5 and 0) → max wins.
        assert_eq!(coverage[1].line, 2);
        assert_eq!(coverage[1].hits, 5);
        // Line 3 has a single uncovered statement → preserved as 0.
        assert_eq!(coverage[2].line, 3);
        assert_eq!(coverage[2].hits, 0);
    }

    #[test]
    fn parse_vitest_results_rejects_malformed_json() {
        assert_eq!(
            parse_vitest_results("not json").unwrap_err().code(),
            "RUNNER_PARSE_ERROR"
        );
    }

    #[test]
    fn parse_istanbul_coverage_flattens_statements() {
        let json = r#"{
            "src/add.ts": {
                "statementMap": {
                    "0": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 30}},
                    "1": {"start": {"line": 2, "column": 2}, "end": {"line": 2, "column": 12}}
                },
                "s": {"0": 3, "1": 0}
            }
        }"#;
        let mut coverage = parse_istanbul_coverage(json).expect("parse");
        coverage.sort_by_key(|c| c.line);
        assert_eq!(coverage.len(), 2);
        assert_eq!(coverage[0].file_path, "src/add.ts");
        assert_eq!(coverage[0].line, 1);
        assert_eq!(coverage[0].hits, 3);
        assert_eq!(coverage[1].line, 2);
        assert_eq!(coverage[1].hits, 0); // uncovered line preserved
    }

    #[test]
    fn parse_vitest_results_caps_attacker_controlled_strings() {
        // A run executes untrusted code that writes results.json, so test
        // names + failure messages must be capped before they reach the DB/UI.
        let name = "n".repeat(MAX_TEST_NAME_BYTES * 2);
        let msg = "m".repeat(MAX_FAILURE_MSG_BYTES * 2);
        let json = format!(
            r#"{{"testResults":[{{"assertionResults":[
                {{"title":"{name}","status":"failed","failureMessages":["{msg}"]}}
            ]}}]}}"#
        );
        let tests = parse_vitest_results(&json).expect("parse");
        assert_eq!(tests.len(), 1);
        assert!(tests[0].name.len() <= MAX_TEST_NAME_BYTES + "…[truncated]".len());
        assert!(tests[0].name.ends_with("…[truncated]"));
        let failure = tests[0].failure_message.as_deref().expect("message");
        assert!(failure.len() <= MAX_FAILURE_MSG_BYTES + "…[truncated]".len());
        assert!(failure.ends_with("…[truncated]"));
    }

    #[test]
    fn reserved_output_paths_are_detected() {
        assert!(is_reserved_output_path("results.json"));
        assert!(is_reserved_output_path("./results.json"));
        assert!(is_reserved_output_path("coverage"));
        assert!(is_reserved_output_path("coverage/coverage-final.json"));
        assert!(is_reserved_output_path("coverage\\coverage-final.json"));
        assert!(!is_reserved_output_path("src/results.json.ts"));
        assert!(!is_reserved_output_path("add.test.ts"));
    }

    /// End-to-end container run. Gated: requires a Docker daemon and the
    /// pre-built `tessera-runner-js` image, so it is `#[ignore]`d and skipped
    /// by the plain unit-test run. CI runs it explicitly in the
    /// `sandbox-runner-test` job (`.github/workflows/ci.yml`), which builds
    /// the image first. Run locally with
    /// `cargo test -- --ignored docker_runner_executes`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires Docker daemon + tessera-runner-js image"]
    async fn docker_runner_executes_a_real_suite() {
        let runner = DockerJsRunner::new();
        let input = RunInput {
            language: RunnerLanguage::TypeScript,
            files: vec![
                WorkspaceFile {
                    relative_path: "src/add.ts".into(),
                    contents: "export const add = (a: number, b: number) => a + b;".into(),
                    is_test: false,
                },
                WorkspaceFile {
                    relative_path: "add.test.ts".into(),
                    contents: "import { test, expect } from 'vitest';\n\
                               import { add } from './src/add';\n\
                               test('adds', () => { expect(add(1, 2)).toBe(3); });"
                        .into(),
                    is_test: true,
                },
            ],
            limits: ResourceLimits::default(),
        };
        let out = runner
            .run(input, CancelToken::new())
            .await
            .expect("container run");
        assert_eq!(out.status, RunStatus::Passed);
        assert!(out.tests.iter().any(|t| t.status == TestStatus::Passed));
        assert!(!out.coverage.is_empty(), "coverage should be captured");
    }
}
