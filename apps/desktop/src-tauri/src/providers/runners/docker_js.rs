//! JS/TS sandboxed test runner backed by local Docker (plan §7).
//!
//! Builds a throwaway workspace from a [`RunInput`], runs the suite inside
//! a hardened, network-isolated container, then parses the vitest results
//! and istanbul coverage the container writes back into the workspace.
//!
//! # Phase boundaries
//!
//! - **Phase 2 (this file):** workspace build, container invocation with
//!   the §7 hardening flags, result/coverage parsing, guaranteed workspace
//!   cleanup. Unit-tested via the pure parser + helper functions only — no
//!   Docker is required to build or test the crate.
//! - **Phase 3 (security gate):** verify `--network none`, wire a real
//!   cancellation token through to `docker kill`, and add a Docker-gated
//!   integration test that actually starts a container.
//! - **Phase 4 (done):** source-line mapping from the vitest reporter
//!   `location`, per-line coverage de-duplication (max hits across the
//!   statements on a line), and fixture-backed parser tests
//!   (`fixtures/*.json`). Branch coverage stays out — `CoverageLine` models
//!   line hits only; adding branches is a separate contract change.
//!
//! The container is deliberately given a non-routable workspace mount and
//! `--network none`: code under test can neither phone home nor reach the
//! host filesystem outside `/work`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;
use uuid::Uuid;

use super::{
    CancelToken, CoverageLine, RunInput, RunStatus, RunnerError, RunnerLanguage, RunnerOutput,
    TestResult, TestRunner, TestStatus,
};

/// Pre-built runner image (plan §7). Built locally on first enable or
/// pulled from a registry — see the Phase 0 ADR. Ships `vitest` + `c8`
/// pre-installed so a run needs no `npm install` (fast, deterministic,
/// offline).
pub const RUNNER_IMAGE: &str = "tessera-runner-js";

/// Workspace mount point inside the container.
const WORK_MOUNT: &str = "/work";

/// Cap on captured stdout / stderr stored or surfaced (§10 — no unbounded
/// blobs). Bytes beyond this are dropped with a truncation marker.
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Per-field caps on parsed result strings. The container writes
/// `results.json`, so test names + failure messages are attacker-controlled
/// (§10 — no unbounded blobs into the DB / UI). Truncated on a char boundary.
const MAX_TEST_NAME_BYTES: usize = 512;
const MAX_FAILURE_MSG_BYTES: usize = 8 * 1024;

/// `--ulimit fsize` cap (bytes): the largest single file the suite may write
/// into the bind-mounted workspace. Bounds a disk-fill `DoS` through `/work`
/// while leaving ample room for `results.json` + coverage on real projects.
const MAX_WRITE_BYTES: u64 = 64 * 1024 * 1024;

/// Filenames the in-container command writes back into the workspace.
const RESULTS_FILE: &str = "results.json";
const COVERAGE_FILE: &str = "coverage/coverage-final.json";

/// Docker-backed JS/TS [`TestRunner`].
#[derive(Debug, Clone, Default)]
pub struct DockerJsRunner {
    /// Root for throwaway workspaces. Defaults to the OS temp dir; tests
    /// can point it elsewhere.
    workspace_root: Option<PathBuf>,
}

impl DockerJsRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn workspace_root(&self) -> PathBuf {
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
        ensure_docker_available().await?;

        // Workspace is removed when `guard` drops — covers the happy path,
        // every `?` early-return, and a panic (§10: always cleaned up).
        let guard = WorkspaceGuard::create(&self.workspace_root())?;
        tracing::debug!(files = input.files.len(), "materializing workspace");
        materialize_workspace(guard.path(), &input)?;

        tracing::debug!(language = ?input.language, "starting container");
        let output = run_container(guard.path(), &input, &cancel).await?;
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

/// Probe for a reachable Docker daemon. Maps a missing binary or a
/// down daemon to [`RunnerError::DockerUnavailable`] so the service can
/// drive the "execution unavailable" UX (plan §3) instead of a hard
/// error.
async fn ensure_docker_available() -> Result<(), RunnerError> {
    let output = Command::new("docker")
        .arg("version")
        .arg("--format")
        .arg("{{.Server.Version}}")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| RunnerError::DockerUnavailable(format!("docker binary not found: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RunnerError::DockerUnavailable(format!(
            "docker daemon unreachable: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

/// Write the source/test files plus a minimal `package.json` and vitest
/// config into the workspace.
/// Paths the runner writes and then reads back after the container exits
/// (`RESULTS_FILE`, the `coverage/` report dir). A crafted artifact must not
/// be allowed to pre-seed these: a container that exits without writing its
/// own output (e.g. a test that hard-kills the process) would otherwise leave
/// the forged file in place and the host would read it as authentic.
fn is_reserved_output_path(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    normalized == RESULTS_FILE || normalized == "coverage" || normalized.starts_with("coverage/")
}

fn materialize_workspace(root: &Path, input: &RunInput) -> Result<(), RunnerError> {
    for file in &input.files {
        if is_reserved_output_path(&file.relative_path) {
            return Err(RunnerError::InvalidInput(format!(
                "workspace file `{}` collides with a runner output path",
                file.relative_path
            )));
        }
        let dest = root.join(&file.relative_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RunnerError::Workspace(format!("create dir {}: {e}", parent.display())))?;
        }
        std::fs::write(&dest, &file.contents)
            .map_err(|e| RunnerError::Workspace(format!("write {}: {e}", dest.display())))?;
    }

    std::fs::write(root.join("package.json"), PACKAGE_JSON)
        .map_err(|e| RunnerError::Workspace(format!("write package.json: {e}")))?;

    let config_name = match input.language {
        RunnerLanguage::TypeScript => "vitest.config.ts",
        RunnerLanguage::JavaScript => "vitest.config.js",
    };
    std::fs::write(root.join(config_name), VITEST_CONFIG)
        .map_err(|e| RunnerError::Workspace(format!("write {config_name}: {e}")))?;

    Ok(())
}

/// Raw result of the container process.
struct ContainerOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

/// Run the suite in a hardened container (plan §7) and capture its output.
///
/// Hardening flags (security gate, §7/§10): `--network none`, CPU / memory /
/// pids caps, `--ulimit fsize` write cap, read-only rootfs + tmpfs,
/// `--cap-drop ALL`, `no-new-privileges`, and a non-root user supplied by the
/// image (`USER` in the runner Dockerfile).
///
/// Termination is the critical part. Dropping the `docker run` child only
/// kills the *CLI*, not the daemon-side container, so on either the
/// wall-clock timeout **or** a user cancellation we issue an explicit
/// `docker kill` against the container's name. `--rm` then reaps it and
/// `kill_on_drop` cleans up the leaked CLI handle.
async fn run_container(
    workspace: &Path,
    input: &RunInput,
    cancel: &CancelToken,
) -> Result<ContainerOutput, RunnerError> {
    let limits = &input.limits;
    let mount = format!("{}:{WORK_MOUNT}", workspace.display());
    let memory = format!("{}m", limits.memory_mb);
    let cpus = format!("{:.2}", f64::from(limits.cpus));
    let pids = limits.pids.to_string();
    let fsize = format!("fsize={MAX_WRITE_BYTES}");
    // Stable handle so the timeout / cancellation paths can target the
    // container directly with `docker kill`.
    let name = format!("tessera-run-{}", Uuid::new_v4());

    let mut cmd = Command::new("docker");
    cmd.arg("run")
        .arg("--rm")
        .args(["--name", &name])
        .args(["--network", "none"])
        .args(["--cpus", &cpus])
        .args(["--memory", &memory])
        .args(["--pids-limit", &pids])
        .args(["--ulimit", &fsize])
        .arg("--read-only")
        .args(["--tmpfs", "/tmp"])
        .args(["--cap-drop", "ALL"])
        .args(["--security-opt", "no-new-privileges"])
        .args(["-v", &mount])
        .args(["-w", WORK_MOUNT])
        .arg(RUNNER_IMAGE)
        .args(["sh", "-c", IN_CONTAINER_CMD])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Backstop: if this future is dropped, SIGKILL the CLI handle too.
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| RunnerError::Process(format!("failed to spawn docker run: {e}")))?;

    let timeout = Duration::from_secs(u64::from(limits.timeout_secs));

    tokio::select! {
        // Completion is checked first so a container that finishes at exactly
        // the wall-clock deadline reports its real results instead of a
        // spurious timeout; cancellation still preempts the timeout below.
        biased;
        result = child.wait_with_output() => {
            let output = result
                .map_err(|e| RunnerError::Process(format!("docker run failed: {e}")))?;
            Ok(ContainerOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            })
        }
        () = cancel.cancelled() => {
            docker_kill(&name).await;
            Err(RunnerError::Cancelled)
        }
        () = tokio::time::sleep(timeout) => {
            docker_kill(&name).await;
            Err(RunnerError::Timeout(limits.timeout_secs))
        }
    }
}

/// Best-effort `docker kill` against a named container. Used on timeout and
/// user cancellation: terminating the local `docker run` process does **not**
/// stop the container running on the daemon, so the daemon must be signalled
/// explicitly. A failure here (e.g. the container already exited) is logged,
/// never propagated — the caller is already returning a terminal error.
async fn docker_kill(name: &str) {
    let result = Command::new("docker")
        .args(["kill", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    if let Err(e) = result {
        tracing::warn!(container = name, error = %e, "failed to docker kill sandbox container");
    }
}

/// Command run inside the container. Emits a vitest JSON report and an
/// istanbul `coverage-final.json`, both into the mounted workspace.
const IN_CONTAINER_CMD: &str = "vitest run --coverage \
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
// Parsers — pure functions, unit-tested below without Docker. Phase 4
// expands the mapping (source lines, branch coverage) against fixtures.
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

/// Saturating `f64 -> u32` for a millisecond duration the caller has
/// already filtered to finite + non-negative.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f64_to_u32(value: f64) -> u32 {
    if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

/// Map parsed assertions to a run-level [`RunStatus`]: any failure →
/// `Failed`; at least one passing test and no failures → `Passed`; nothing
/// executed → `Error`.
fn derive_status(tests: &[TestResult]) -> RunStatus {
    if tests.iter().any(|t| t.status == TestStatus::Failed) {
        return RunStatus::Failed;
    }
    if tests.iter().any(|t| t.status == TestStatus::Passed) {
        return RunStatus::Passed;
    }
    RunStatus::Error
}

/// Truncate captured stdout/stderr to [`MAX_OUTPUT_BYTES`].
fn truncate(s: &str) -> String {
    truncate_to(s, MAX_OUTPUT_BYTES)
}

/// Truncate `s` to at most `max` bytes on a char boundary, appending a
/// marker when bytes were dropped. Shared by the output cap and the
/// per-field result-string caps.
fn truncate_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &s[..end])
}

/// RAII guard for the throwaway workspace. Removing on `Drop` guarantees
/// cleanup on the happy path, on any `?` early-return, and on panic (§10).
struct WorkspaceGuard {
    path: PathBuf,
}

impl WorkspaceGuard {
    fn create(root: &Path) -> Result<Self, RunnerError> {
        let path = root.join(format!("tessera-run-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path)
            .map_err(|e| RunnerError::Workspace(format!("create workspace {}: {e}", path.display())))?;
        // The container runs as a non-root user (image `USER`), so it must be
        // able to write `results.json` + coverage back into the bind-mounted
        // workspace. Making the throwaway dir group/other-writable also keeps
        // host-side cleanup working (a root-owned file would defeat the Drop
        // remover). The dir lives under the per-user temp root for one run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777)).map_err(
                |e| RunnerError::Workspace(format!("chmod workspace {}: {e}", path.display())),
            )?;
        }
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.path) {
            // Best-effort: a failed cleanup must not mask the run result,
            // but it is worth a warning for disk-leak diagnosis.
            tracing::warn!(
                workspace = %self.path.display(),
                error = %e,
                "failed to remove sandbox workspace"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::runners::{ResourceLimits, WorkspaceFile};

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
    fn derive_status_prioritizes_failure() {
        let failed = vec![
            TestResult { name: "a".into(), status: TestStatus::Passed, duration_ms: 1, failure_message: None, source_line: None },
            TestResult { name: "b".into(), status: TestStatus::Failed, duration_ms: 1, failure_message: None, source_line: None },
        ];
        assert_eq!(derive_status(&failed), RunStatus::Failed);

        let all_pass = vec![TestResult { name: "a".into(), status: TestStatus::Passed, duration_ms: 1, failure_message: None, source_line: None }];
        assert_eq!(derive_status(&all_pass), RunStatus::Passed);

        assert_eq!(derive_status(&[]), RunStatus::Error);
    }

    #[test]
    fn truncate_caps_long_output() {
        let big = "a".repeat(MAX_OUTPUT_BYTES + 100);
        let out = truncate(&big);
        assert!(out.len() < big.len());
        assert!(out.ends_with("…[truncated]"));

        let small = "short";
        assert_eq!(truncate(small), "short");
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

    /// End-to-end container run. Gated: requires a Docker daemon and the
    /// pre-built `tessera-runner-js` image, so it is `#[ignore]`d and skips in
    /// CI. Run locally with `cargo test -- --ignored docker_runner_executes`.
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
