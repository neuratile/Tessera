//! Python sandboxed test runner backed by local Docker
//! (`plan/SANDBOX_PYTHON_RUNNER.md`).
//!
//! Second vertical slice of the sandboxed runner: same [`TestRunner`]
//! trait, same hardened container ([`docker_harness`]), different
//! toolchain — pytest (via `pytest-json-report`) for results and
//! coverage.py (`coverage json`) for line coverage. Only stdlib + pytest
//! are available inside the image (plan §2): a generated test importing
//! `requests` etc. fails with a readable `ModuleNotFoundError` in the
//! results panel — the network is off by design, so nothing can be
//! installed at run time anyway.
//!
//! This file owns only what is Python-specific: the image name, the
//! in-container pytest/coverage invocation, and the two parsers. All
//! hardening flags, timeout/cancel handling, and workspace lifecycle come
//! from the shared harness, so this runner cannot drift to a weaker
//! sandbox than the JS one.

use async_trait::async_trait;
use serde::Deserialize;

use super::docker_harness::{
    self, derive_status, f64_to_u32, truncate, truncate_to, ContainerOutput, WorkspaceGuard,
    MAX_FAILURE_MSG_BYTES, MAX_TEST_NAME_BYTES,
};
use super::{
    CancelToken, CoverageLine, RunInput, RunnerError, RunnerOutput, TestResult, TestRunner,
    TestStatus,
};

/// Pre-built runner image. Built locally from `docker/Dockerfile.runner-py`,
/// never pulled from a registry (local-first guarantee — ADR-0004). Ships
/// pinned `pytest` + `pytest-json-report` + `coverage` so a run needs no
/// `pip install` (which `--network none` forbids anyway).
pub const RUNNER_IMAGE: &str = "tessera-runner-py";

/// Dockerfile (under `apps/desktop/src-tauri/docker/`) the image is built
/// from; surfaced in the `ImageMissing` build hint.
const RUNNER_DOCKERFILE: &str = "Dockerfile.runner-py";

/// Filenames the in-container command writes back into the workspace.
const RESULTS_FILE: &str = "results.json";
const COVERAGE_FILE: &str = "coverage/coverage.json";
/// coverage.py's intermediate data file (cwd-relative inside `/work`).
const COVERAGE_DATA_FILE: &str = ".coverage";

/// Command run inside the container (plan §6). `coverage run -m pytest`
/// records line coverage while pytest-json-report writes the results JSON;
/// `coverage json` then runs even when tests fail (`;`, not `&&`) so failed
/// runs still report coverage, matching the JS runner. The pytest cache
/// plugin is disabled to keep the bind-mounted workspace free of junk.
const IN_CONTAINER_CMD: &str = "coverage run -m pytest . -q -p no:cacheprovider \
     --json-report --json-report-file=results.json ; \
     mkdir -p coverage && coverage json -o coverage/coverage.json";

/// Docker-backed Python [`TestRunner`].
#[derive(Debug, Clone, Default)]
pub struct DockerPyRunner {
    /// Root for throwaway workspaces. Defaults to the OS temp dir; tests
    /// can point it elsewhere.
    workspace_root: Option<std::path::PathBuf>,
}

impl DockerPyRunner {
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
impl TestRunner for DockerPyRunner {
    fn name(&self) -> &'static str {
        "docker-py"
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
        docker_harness::materialize_files(guard.path(), &input, is_reserved_output_path)?;

        tracing::debug!(language = ?input.language, "starting container");
        let output: ContainerOutput =
            docker_harness::run_container(guard.path(), RUNNER_IMAGE, IN_CONTAINER_CMD, &input, &cancel)
                .await?;
        let stdout = truncate(&output.stdout);
        let stderr = truncate(&output.stderr);

        let results_path = guard.path().join(RESULTS_FILE);
        let coverage_path = guard.path().join(COVERAGE_FILE);

        // pytest exits non-zero when assertions fail — that is a normal
        // `Failed` run, not a process error. Only treat a missing results
        // file as a genuine failure.
        let results_json = std::fs::read_to_string(&results_path).map_err(|e| {
            RunnerError::Process(format!(
                "pytest produced no results file ({e}); container exit {}, stderr: {}",
                output.exit_code, stderr
            ))
        })?;

        let tests = parse_pytest_results(&results_json)?;
        let coverage = std::fs::read_to_string(&coverage_path)
            .ok()
            .map(|json| parse_coverage_py(&json))
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
/// (`RESULTS_FILE`, the `coverage/` report dir, coverage.py's `.coverage`
/// data file). A crafted artifact must not be allowed to pre-seed these —
/// see `docker_harness::materialize_files`.
fn is_reserved_output_path(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    normalized == RESULTS_FILE
        || normalized == COVERAGE_DATA_FILE
        || normalized == "coverage"
        || normalized.starts_with("coverage/")
}

// ---------------------------------------------------------------------------
// Parsers — pure functions, unit-tested below without Docker against the
// committed fixtures (`fixtures/pytest-report.json`,
// `fixtures/coverage-py.json`).
// ---------------------------------------------------------------------------

/// Subset of the pytest-json-report shape we consume.
#[derive(Debug, Deserialize)]
struct PytestReport {
    #[serde(default)]
    tests: Vec<PytestTest>,
}

#[derive(Debug, Deserialize)]
struct PytestTest {
    #[serde(default)]
    nodeid: String,
    /// 0-based line of the test function in its file (pytest `location`).
    #[serde(default)]
    lineno: Option<u32>,
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    setup: Option<PytestStage>,
    #[serde(default)]
    call: Option<PytestStage>,
    #[serde(default)]
    teardown: Option<PytestStage>,
}

#[derive(Debug, Deserialize)]
struct PytestStage {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    crash: Option<PytestCrash>,
}

#[derive(Debug, Deserialize)]
struct PytestCrash {
    #[serde(default)]
    message: Option<String>,
}

/// Parse pytest-json-report output into [`TestResult`]s.
///
/// - `outcome` mapping: `passed` → Passed; `failed` / `error` (setup or
///   teardown blew up — e.g. a `ModuleNotFoundError` on import) → Failed;
///   everything else (`skipped`, `xfailed`, `xpassed`) → Skipped.
/// - `duration_ms` comes from the `call` stage (seconds, f64); a test that
///   never reached `call` reports 0.
/// - The failure message is the first crash across `call` → `setup` →
///   `teardown`, truncated (attacker-controlled, §10).
/// - `lineno` is 0-based in the report; persisted `source_line` is 1-based.
///
/// # Errors
///
/// [`RunnerError::Parse`] when the JSON is malformed.
pub fn parse_pytest_results(json: &str) -> Result<Vec<TestResult>, RunnerError> {
    let report: PytestReport = serde_json::from_str(json)
        .map_err(|e| RunnerError::Parse(format!("pytest report: {e}")))?;

    let mut tests = Vec::new();
    for test in report.tests {
        let status = match test.outcome.as_str() {
            "passed" => TestStatus::Passed,
            "failed" | "error" => TestStatus::Failed,
            // "skipped" | "xfailed" | "xpassed" | "deselected"
            _ => TestStatus::Skipped,
        };
        let name = truncate_to(&pytest_display_name(&test.nodeid), MAX_TEST_NAME_BYTES);
        let failure_message = [&test.call, &test.setup, &test.teardown]
            .into_iter()
            .filter_map(|stage| stage.as_ref())
            .filter_map(|stage| stage.crash.as_ref())
            .filter_map(|crash| crash.message.as_deref())
            .find(|m| !m.trim().is_empty())
            .map(|m| truncate_to(m, MAX_FAILURE_MSG_BYTES));
        let duration_ms = test
            .call
            .as_ref()
            .and_then(|stage| stage.duration)
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map_or(0, |secs| f64_to_u32(secs * 1000.0));
        let source_line = test.lineno.map(|line| line.saturating_add(1));
        tests.push(TestResult {
            name,
            status,
            duration_ms,
            failure_message,
            source_line,
        });
    }
    Ok(tests)
}

/// Render a pytest `nodeid` as a display name that the sandbox name→id
/// bridge (`sandbox_service::parse_case_id`) can fold back onto its test
/// case.
///
/// Python identifiers cannot carry the JS spec-title convention
/// (`'TC-LOGIN-01 rejects empty password'`), so the prompt mandates
/// lower-snake function names with a double-underscore separator between
/// the case id and the description:
/// `test_tc_login_01__rejects_empty_password`. This function re-hyphenates
/// and uppercases that token — `TC-LOGIN-01 rejects empty password` — so
/// the existing service-side `^TC-[A-Z0-9_-]+` extraction works unchanged.
/// Names not following the convention are passed through as the raw nodeid.
fn pytest_display_name(nodeid: &str) -> String {
    // `tests/test_add.py::TestAdd::test_tc_add_01__adds[2-3]` → function
    // part is the last `::` segment; a parametrize suffix is preserved.
    let function = nodeid.rsplit("::").next().unwrap_or(nodeid);
    let (function, params) = match function.split_once('[') {
        Some((f, p)) => (f, Some(p)),
        None => (function, None),
    };
    let Some(rest) = function.strip_prefix("test_") else {
        return nodeid.to_string();
    };
    if !rest.to_ascii_lowercase().starts_with("tc_") {
        return nodeid.to_string();
    }
    let (id_part, description) = match rest.split_once("__") {
        Some((id, desc)) => (id, desc),
        None => (rest, ""),
    };
    let case_id = id_part.to_ascii_uppercase().replace('_', "-");
    let mut name = case_id;
    if !description.is_empty() {
        name.push(' ');
        name.push_str(&description.replace('_', " "));
    }
    if let Some(p) = params {
        name.push_str(" [");
        name.push_str(p);
    }
    name
}

/// Flatten a `coverage json` report into per-line hit entries.
///
/// coverage.py reports `executed_lines` / `missing_lines` per file, not hit
/// counts, so executed lines are recorded as `hits = 1` and missing lines as
/// `hits = 0`. The frontend gutters only distinguish `hits == 0` vs `> 0`,
/// so the loss of counts is invisible (plan §6). File paths are workspace
/// relative (coverage runs with cwd `/work`); the editor keys coverage by
/// both relative and `/work/…` forms. Output is sorted by file then line
/// (`BTreeMap` order), matching `test_run_repo::fetch_run` read-back.
///
/// # Errors
///
/// [`RunnerError::Parse`] when the JSON is malformed.
pub fn parse_coverage_py(json: &str) -> Result<Vec<CoverageLine>, RunnerError> {
    let report: CoveragePyReport = serde_json::from_str(json)
        .map_err(|e| RunnerError::Parse(format!("coverage.py report: {e}")))?;

    let mut by_line: std::collections::BTreeMap<(String, u32), u32> =
        std::collections::BTreeMap::new();
    for (file_path, file) in report.files {
        for line in file.executed_lines {
            if line == 0 {
                continue;
            }
            by_line
                .entry((file_path.clone(), line))
                .and_modify(|h| *h = (*h).max(1))
                .or_insert(1);
        }
        for line in file.missing_lines {
            if line == 0 {
                continue;
            }
            by_line.entry((file_path.clone(), line)).or_insert(0);
        }
    }

    Ok(by_line
        .into_iter()
        .map(|((file_path, line), hits)| CoverageLine { file_path, line, hits })
        .collect())
}

#[derive(Debug, Deserialize)]
struct CoveragePyReport {
    #[serde(default)]
    files: std::collections::BTreeMap<String, CoveragePyFile>,
}

#[derive(Debug, Deserialize)]
struct CoveragePyFile {
    #[serde(default)]
    executed_lines: Vec<u32>,
    #[serde(default)]
    missing_lines: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::runners::{ResourceLimits, RunStatus, RunnerLanguage, WorkspaceFile};

    #[test]
    fn pytest_display_name_rehyphenates_tc_tokens() {
        assert_eq!(
            pytest_display_name("test_add.py::test_tc_login_01__rejects_empty_password"),
            "TC-LOGIN-01 rejects empty password"
        );
        // No description part → bare id.
        assert_eq!(pytest_display_name("test_add.py::test_tc_add_02"), "TC-ADD-02");
        // Class-based nodeids use the last segment.
        assert_eq!(
            pytest_display_name("tests/test_x.py::TestAdd::test_tc_a__b_c"),
            "TC-A b c"
        );
        // Parametrize suffix is preserved on the display name.
        assert_eq!(
            pytest_display_name("test_add.py::test_tc_a_1__adds[2-3]"),
            "TC-A-1 adds [2-3]"
        );
        // Non-convention names pass through as the raw nodeid.
        assert_eq!(
            pytest_display_name("test_add.py::test_plain_helper"),
            "test_add.py::test_plain_helper"
        );
        assert_eq!(pytest_display_name("weird"), "weird");
    }

    #[test]
    fn parse_pytest_results_maps_outcomes_durations_and_lines() {
        let json = r#"{
            "tests": [
                {"nodeid": "test_add.py::test_tc_add_01__adds", "lineno": 4, "outcome": "passed",
                 "setup": {"duration": 0.001}, "call": {"duration": 0.0123}, "teardown": {"duration": 0.0001}},
                {"nodeid": "test_add.py::test_tc_add_02__rejects", "lineno": 9, "outcome": "failed",
                 "setup": {"duration": 0.001},
                 "call": {"duration": 0.004, "crash": {"message": "assert 2 == 3"}}},
                {"nodeid": "test_add.py::test_tc_add_03__skipped_case", "lineno": 14, "outcome": "skipped"}
            ]
        }"#;
        let tests = parse_pytest_results(json).expect("parse");
        assert_eq!(tests.len(), 3);

        assert_eq!(tests[0].name, "TC-ADD-01 adds");
        assert_eq!(tests[0].status, TestStatus::Passed);
        assert_eq!(tests[0].duration_ms, 12); // 0.0123 s → 12 ms (truncated)
        assert_eq!(tests[0].source_line, Some(5)); // 0-based lineno + 1
        assert!(tests[0].failure_message.is_none());

        assert_eq!(tests[1].status, TestStatus::Failed);
        assert_eq!(tests[1].duration_ms, 4);
        assert_eq!(tests[1].failure_message.as_deref(), Some("assert 2 == 3"));

        assert_eq!(tests[2].status, TestStatus::Skipped);
        assert_eq!(tests[2].duration_ms, 0); // never reached `call`
    }

    #[test]
    fn parse_pytest_results_maps_error_outcome_to_failed_with_setup_crash() {
        // A test importing an unavailable third-party module dies in
        // collection/setup with outcome "error" — the plan's acceptance
        // criterion 4: surfaced as a readable failure, not a hang or crash.
        let json = r#"{
            "tests": [
                {"nodeid": "test_api.py::test_tc_api_01__fetches", "lineno": 2, "outcome": "error",
                 "setup": {"duration": 0.001, "crash": {"message": "ModuleNotFoundError: No module named 'requests'"}}}
            ]
        }"#;
        let tests = parse_pytest_results(json).expect("parse");
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].status, TestStatus::Failed);
        assert_eq!(
            tests[0].failure_message.as_deref(),
            Some("ModuleNotFoundError: No module named 'requests'")
        );
        assert_eq!(tests[0].duration_ms, 0);
    }

    #[test]
    fn parse_pytest_results_fixture_round_trips() {
        let json = include_str!("fixtures/pytest-report.json");
        let tests = parse_pytest_results(json).expect("parse fixture");
        assert_eq!(tests.len(), 4);

        assert_eq!(tests[0].name, "TC-ADD-01 adds two positive numbers");
        assert_eq!(tests[0].status, TestStatus::Passed);
        assert_eq!(tests[0].source_line, Some(5));

        assert_eq!(tests[1].name, "TC-ADD-02 adds negatives");
        assert_eq!(tests[1].status, TestStatus::Passed);

        assert_eq!(tests[2].name, "TC-ADD-03 rejects non numeric input");
        assert_eq!(tests[2].status, TestStatus::Failed);
        assert!(
            tests[2]
                .failure_message
                .as_deref()
                .expect("failure message")
                .contains("TypeError"),
        );

        assert_eq!(tests[3].status, TestStatus::Skipped);
    }

    #[test]
    fn parse_pytest_results_rejects_malformed_json() {
        assert_eq!(
            parse_pytest_results("not json").unwrap_err().code(),
            "RUNNER_PARSE_ERROR"
        );
    }

    #[test]
    fn parse_pytest_results_caps_attacker_controlled_strings() {
        // results.json is written by untrusted code under test, so names +
        // messages must be capped before they reach the DB/UI (§10).
        let name = "n".repeat(MAX_TEST_NAME_BYTES * 2);
        let msg = "m".repeat(MAX_FAILURE_MSG_BYTES * 2);
        let json = format!(
            r#"{{"tests":[{{"nodeid":"{name}","outcome":"failed",
                "call":{{"crash":{{"message":"{msg}"}}}}}}]}}"#
        );
        let tests = parse_pytest_results(&json).expect("parse");
        assert_eq!(tests.len(), 1);
        assert!(tests[0].name.len() <= MAX_TEST_NAME_BYTES + "…[truncated]".len());
        assert!(tests[0].name.ends_with("…[truncated]"));
        let failure = tests[0].failure_message.as_deref().expect("message");
        assert!(failure.len() <= MAX_FAILURE_MSG_BYTES + "…[truncated]".len());
        assert!(failure.ends_with("…[truncated]"));
    }

    #[test]
    fn parse_coverage_py_maps_executed_and_missing_to_hits() {
        let json = r#"{
            "files": {
                "src/add.py": {
                    "executed_lines": [1, 2, 4],
                    "missing_lines": [6]
                }
            }
        }"#;
        let coverage = parse_coverage_py(json).expect("parse");
        assert_eq!(coverage.len(), 4);
        assert!(coverage.iter().all(|c| c.file_path == "src/add.py"));
        assert_eq!(coverage[0].line, 1);
        assert_eq!(coverage[0].hits, 1);
        assert_eq!(coverage[3].line, 6);
        assert_eq!(coverage[3].hits, 0); // missing line preserved as uncovered
    }

    #[test]
    fn parse_coverage_py_fixture_round_trips() {
        let json = include_str!("fixtures/coverage-py.json");
        let coverage = parse_coverage_py(json).expect("parse fixture");

        let add_lines: Vec<_> = coverage
            .iter()
            .filter(|c| c.file_path == "src/add.py")
            .collect();
        assert!(!add_lines.is_empty());
        // Executed lines report hits = 1, missing lines hits = 0.
        assert!(add_lines.iter().any(|c| c.hits == 1));
        assert!(add_lines.iter().any(|c| c.hits == 0));
        // Sorted by file then line.
        let mut sorted = coverage.clone();
        sorted.sort_by(|a, b| a.file_path.cmp(&b.file_path).then(a.line.cmp(&b.line)));
        assert_eq!(
            coverage.iter().map(|c| (&c.file_path, c.line)).collect::<Vec<_>>(),
            sorted.iter().map(|c| (&c.file_path, c.line)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn parse_coverage_py_rejects_malformed_json() {
        assert_eq!(
            parse_coverage_py("not json").unwrap_err().code(),
            "RUNNER_PARSE_ERROR"
        );
    }

    #[test]
    fn reserved_output_paths_are_detected() {
        assert!(is_reserved_output_path("results.json"));
        assert!(is_reserved_output_path("./results.json"));
        assert!(is_reserved_output_path(".coverage"));
        assert!(is_reserved_output_path("coverage"));
        assert!(is_reserved_output_path("coverage/coverage.json"));
        assert!(is_reserved_output_path("coverage\\coverage.json"));
        assert!(!is_reserved_output_path("src/coverage_utils.py"));
        assert!(!is_reserved_output_path("test_add.py"));
    }

    /// End-to-end container run (mirror of `docker_runner_executes_a_real_suite`).
    /// Gated: requires a Docker daemon and the pre-built `tessera-runner-py`
    /// image, so it is `#[ignore]`d and skipped by the plain unit-test run.
    /// CI runs it explicitly in the `sandbox-runner-test` job, which builds
    /// the image first. Run locally with
    /// `cargo test -- --ignored docker_py_runner_executes`.
    ///
    /// Also asserts the §9 security checklist's network gate: a socket
    /// connect attempt from inside the container must fail (`--network none`).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires Docker daemon + tessera-runner-py image"]
    async fn docker_py_runner_executes_a_real_suite() {
        let runner = DockerPyRunner::new();
        let input = RunInput {
            language: RunnerLanguage::Python,
            files: vec![
                WorkspaceFile {
                    relative_path: "src/add.py".into(),
                    contents: "def add(a, b):\n    return a + b\n".into(),
                    is_test: false,
                },
                WorkspaceFile {
                    relative_path: "test_add.py".into(),
                    contents: "\
import socket

import pytest

from src.add import add


def test_tc_add_01__adds_two_numbers():
    assert add(1, 2) == 3


def test_tc_add_02__fails_on_purpose():
    assert add(1, 2) == 4


def test_tc_net_01__network_is_unreachable():
    with pytest.raises(OSError):
        s = socket.create_connection(('1.1.1.1', 80), timeout=2)
        s.close()
"
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

        // One deliberate failure → run is Failed, with passes alongside.
        assert_eq!(out.status, RunStatus::Failed);
        assert!(out
            .tests
            .iter()
            .any(|t| t.name == "TC-ADD-01 adds two numbers" && t.status == TestStatus::Passed));
        assert!(out
            .tests
            .iter()
            .any(|t| t.name == "TC-ADD-02 fails on purpose" && t.status == TestStatus::Failed));
        // `--network none`: the connect attempt must raise, so the test passes.
        assert!(out
            .tests
            .iter()
            .any(|t| t.name == "TC-NET-01 network is unreachable"
                && t.status == TestStatus::Passed));
        // Coverage captured on the source under test even though the run failed.
        assert!(
            out.coverage.iter().any(|c| c.file_path.contains("add.py")),
            "coverage should be captured"
        );
    }
}
