//! Sandboxed test-runner abstraction + shared wire types.
//!
//! Phase 1 (contract slice, `plan/SANDBOX_TEST_RUNNER.md` §6): the serde
//! structs + status enums that cross the IPC boundary and persist to the
//! `test_runs` family of tables (migration `0004_test_runs.sql`). They
//! mirror the Zod schemas in
//! `packages/shared/src/schemas/test-run.schema.ts` — Rust serde is the
//! source of truth (`rules.md` §12.3.1), Zod follows.
//!
//! Phase 2 adds the [`TestRunner`] async trait + the Docker implementation
//! ([`docker_js`]). The trait keeps `sandbox_service` ignorant of Docker
//! specifics so a cloud impl (plan §11) can drop in later.
//!
//! Wire convention mirrors the rest of the IPC layer: structs serialize
//! `camelCase`; the status enums serialize `snake_case` (which, for these
//! single-word variants, is plain lowercase — matching the Zod literals
//! and the TEXT stored in the `status` columns).

pub mod docker_js;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Notify;

/// Lifecycle state of a single sandboxed run. Mirrors the `RunStatus`
/// literals in `test-run.schema.ts` and the `test_runs.status` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Row created, container not yet started.
    Pending,
    /// Container is executing.
    Running,
    /// Every executed assertion passed.
    Passed,
    /// At least one assertion failed (the run itself completed cleanly).
    Failed,
    /// The run could not complete (Docker absent, build failure, timeout).
    Error,
    /// User stopped the run; the container was killed.
    Cancelled,
}

impl RunStatus {
    /// Stable string used in DB rows and IPC payloads. Matches the serde
    /// `snake_case` wire form so storage and transport stay lossless.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }

    /// Inverse of [`as_str`](Self::as_str), used by the repository when
    /// decoding rows. Returns `None` for any unrecognised string.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "error" => Some(Self::Error),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Outcome of a single executed assertion. Mirrors the `TestStatus`
/// literals in `test-run.schema.ts` and the `test_run_cases.status`
/// column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

impl TestStatus {
    /// Stable string used in DB rows and IPC payloads.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    /// Inverse of [`as_str`](Self::as_str). Returns `None` for any
    /// unrecognised string.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

/// IPC request to execute a generated test-case artifact in the sandbox.
/// Mirrors `RunRequestSchema`. `opt_in_confirmed` must be `true`; the
/// backend rejects runs when execution is opted out (plan §3 — defence in
/// depth, not just a hidden UI button).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub artifact_id: String,
    pub opt_in_confirmed: bool,
    /// Caller-generated correlation id (UUID) the run registers its cancel
    /// token under, so the UI can Stop a run it has not yet seen the result
    /// of (the run IPC only returns once the run finishes). Defaults to
    /// empty when absent — such a run is simply not cancellable.
    #[serde(default)]
    pub client_run_id: String,
}

/// One executed test assertion. Mirrors `TestResultSchema`.
///
/// `failure_message` and `source_line` are `None` for passing/skipped
/// cases; when `None` they are omitted from the JSON payload so the wire
/// shape matches the Zod `.optional()` fields exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub name: String,
    pub status: TestStatus,
    pub duration_ms: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
}

/// Coverage hit-count for one source line. Mirrors `CoverageLineSchema`.
/// `hits == 0` marks an uncovered line. `line` is 1-based.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageLine {
    pub file_path: String,
    pub line: u32,
    pub hits: u32,
}

/// Aggregate result of a run, returned to the renderer and persisted.
/// Mirrors `RunResultSchema`. `error_message` is omitted from the wire
/// payload when `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub run_id: String,
    pub status: RunStatus,
    pub passed_count: u32,
    pub failed_count: u32,
    pub duration_ms: u32,
    pub tests: Vec<TestResult>,
    pub coverage: Vec<CoverageLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Execution contract (Phase 2). These types never cross the IPC boundary —
// they are the internal handoff between `sandbox_service` and a concrete
// `TestRunner`. Only [`RunResult`] / [`TestResult`] / [`CoverageLine`] above
// are serialized to the renderer.
// ---------------------------------------------------------------------------

/// Source language a runner executes. Phase 2 ships JS/TS; `docker_py`
/// (plan §11, Phase 6) adds Python behind the same trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerLanguage {
    JavaScript,
    TypeScript,
}

impl RunnerLanguage {
    /// Detect from a file extension. Defaults to [`Self::TypeScript`] for
    /// `.ts`/`.tsx`/`.mts`/`.cts` (case-insensitive), otherwise
    /// [`Self::JavaScript`].
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if ["ts", "tsx", "mts", "cts"]
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        {
            Self::TypeScript
        } else {
            Self::JavaScript
        }
    }
}

/// One file written into the throwaway workspace before the container
/// starts. `relative_path` must stay inside the workspace root — enforced
/// by [`RunInput::validate`] (no absolute paths, no `..` traversal).
#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub relative_path: String,
    pub contents: String,
    /// `true` for a generated test (vitest spec); `false` for the
    /// source-under-test. Lets the runner target the test glob.
    pub is_test: bool,
}

/// Resource caps applied to the sandbox container (plan §7). Carried from
/// Phase 2 so the contract is stable; the Docker flags that enforce them
/// are applied in [`docker_js`] and verified in the Phase 3 security gate.
#[derive(Debug, Clone, Copy)]
pub struct ResourceLimits {
    /// `--cpus` fractional CPU cap.
    pub cpus: f32,
    /// `--memory` cap in MiB.
    pub memory_mb: u32,
    /// `--pids-limit` process cap.
    pub pids: u32,
    /// Wall-clock timeout in seconds → `docker kill` on expiry.
    pub timeout_secs: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpus: 1.0,
            memory_mb: 512,
            pids: 256,
            timeout_secs: 60,
        }
    }
}

/// Hard caps on a single run's workspace. Defence against a malicious or
/// runaway artifact trying to exhaust host disk / memory before the
/// container even starts (security gate, plan §10). Enforced by
/// [`RunInput::validate`].
pub const MAX_WORKSPACE_FILES: usize = 200;
/// Total byte budget across every file's `contents`.
pub const MAX_WORKSPACE_BYTES: usize = 8 * 1024 * 1024;

/// Everything a [`TestRunner`] needs to execute one run.
#[derive(Debug, Clone)]
pub struct RunInput {
    pub language: RunnerLanguage,
    pub files: Vec<WorkspaceFile>,
    pub limits: ResourceLimits,
}

impl RunInput {
    /// Validate the workspace is safe to materialize: a bounded number of
    /// files within a total byte budget, at least one test file, and every
    /// `relative_path` confined to the workspace root.
    ///
    /// This is the first line of the §10 "no path traversal into the
    /// host" guard; the runner also refuses absolute mounts.
    ///
    /// # Errors
    ///
    /// [`RunnerError::InvalidInput`] when no files are supplied, the file
    /// count or total size exceeds the caps, no test file is present, or any
    /// path is absolute / contains a `..` component / is empty.
    pub fn validate(&self) -> Result<(), RunnerError> {
        if self.files.is_empty() {
            return Err(RunnerError::InvalidInput("no workspace files supplied".into()));
        }
        if self.files.len() > MAX_WORKSPACE_FILES {
            return Err(RunnerError::InvalidInput(format!(
                "too many workspace files: {} (max {MAX_WORKSPACE_FILES})",
                self.files.len()
            )));
        }
        let total_bytes: usize = self.files.iter().map(|f| f.contents.len()).sum();
        if total_bytes > MAX_WORKSPACE_BYTES {
            return Err(RunnerError::InvalidInput(format!(
                "workspace too large: {total_bytes} bytes (max {MAX_WORKSPACE_BYTES})"
            )));
        }
        if !self.files.iter().any(|f| f.is_test) {
            return Err(RunnerError::InvalidInput(
                "no test file in workspace (expected at least one is_test file)".into(),
            ));
        }
        for file in &self.files {
            if !is_safe_relative_path(&file.relative_path) {
                return Err(RunnerError::InvalidInput(format!(
                    "unsafe workspace path `{}`",
                    file.relative_path
                )));
            }
        }
        Ok(())
    }
}

/// Cooperative cancellation signal handed to a [`TestRunner`]. The runner
/// races the suite against [`cancelled`](CancelToken::cancelled) and, on
/// fire, kills the container (plan §7 — "cancellation token wired through →
/// `docker kill` on user Stop").
///
/// The wall-clock timeout lives inside the runner itself; this token is the
/// *user-initiated* Stop path. Phase 3 plumbs it end to end so the kill path
/// exists and is exercised by tests; Phase 5 wires the UI Stop button to a
/// per-run [`cancel`](CancelToken::cancel) via a run registry.
#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation. Idempotent; wakes any task awaiting
    /// [`cancelled`](Self::cancelled).
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// `true` once [`cancel`](Self::cancel) has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolve once cancellation is requested; pends forever otherwise.
    pub async fn cancelled(&self) {
        let notified = self.notify.notified();
        tokio::pin!(notified);
        // Register interest *before* re-checking the flag so a cancel that
        // races between the check and the await cannot be lost.
        notified.as_mut().enable();
        if self.flag.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }
}

/// Reject absolute paths, empty paths, Windows drive prefixes, and any
/// component that is `..` (parent traversal) so a malicious or buggy
/// artifact cannot escape the workspace mount onto the host.
fn is_safe_relative_path(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    // Absolute (Unix `/…`, Windows `\…` or `C:\…`).
    if path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return false;
    }
    path.split(['/', '\\'])
        .all(|component| component != ".." && !component.is_empty())
}

/// What a runner produces before persistence. `sandbox_service` maps this
/// into the persisted [`RunResult`]; `stdout`/`stderr` are truncated by
/// the runner before they reach here (§10 — no unbounded blobs).
#[derive(Debug, Clone)]
pub struct RunnerOutput {
    pub status: RunStatus,
    pub tests: Vec<TestResult>,
    pub coverage: Vec<CoverageLine>,
    pub stdout: String,
    pub stderr: String,
}

/// Typed failures a [`TestRunner`] can surface. Distinct variants exist
/// only where `sandbox_service` (or the UX) treats them differently —
/// `DockerUnavailable` drives the "feature unavailable" path (plan §3),
/// `Timeout` / `Cancelled` are expected terminal states, the rest are
/// genuine errors.
#[derive(Debug, Error)]
pub enum RunnerError {
    /// Docker binary missing or daemon unreachable. Drives the
    /// "execution unavailable" UX rather than a hard error (plan §3).
    #[error("docker unavailable: {0}")]
    DockerUnavailable(String),

    /// The pre-built runner image is absent from the local daemon. The image
    /// is built locally, never pulled from a registry (local-first guarantee),
    /// so without this preflight a missing image surfaces as a cryptic
    /// registry-pull failure from `docker run`. Carries the build command.
    #[error("runner image missing: {0}")]
    ImageMissing(String),

    /// The supplied [`RunInput`] failed validation (empty / unsafe path /
    /// no test file).
    #[error("invalid run input: {0}")]
    InvalidInput(String),

    /// Wall-clock timeout elapsed; the container was killed.
    #[error("runner timed out after {0}s")]
    Timeout(u32),

    /// User stopped the run (Phase 3 cancellation wiring).
    #[error("runner cancelled")]
    Cancelled,

    /// Failed to build / clean the temp workspace.
    #[error("workspace error: {0}")]
    Workspace(String),

    /// Runner output (vitest / istanbul JSON) could not be parsed.
    #[error("failed to parse runner output: {0}")]
    Parse(String),

    /// The runner process failed for a reason not covered above.
    #[error("runner process failed: {0}")]
    Process(String),
}

impl RunnerError {
    /// Stable identifier for logs and the IPC error boundary.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::DockerUnavailable(_) => "DOCKER_UNAVAILABLE",
            Self::ImageMissing(_) => "RUNNER_IMAGE_MISSING",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::Timeout(_) => "RUNNER_TIMEOUT",
            Self::Cancelled => "RUNNER_CANCELLED",
            Self::Workspace(_) => "RUNNER_WORKSPACE_ERROR",
            Self::Parse(_) => "RUNNER_PARSE_ERROR",
            Self::Process(_) => "RUNNER_PROCESS_ERROR",
        }
    }
}

/// Sandboxed test-runner abstraction. Mirrors the `LlmProvider` pattern
/// (`rules.md` §5.2): `sandbox_service` depends on this trait, never on a
/// concrete runner. Phase 2 ships [`docker_js::DockerJsRunner`]; a cloud
/// runner (plan §11) can implement the same trait later.
#[async_trait]
pub trait TestRunner: Send + Sync {
    /// Stable identifier stored in `test_runs.runner` and used in logs.
    /// Lowercase kebab-case (`docker-js`).
    fn name(&self) -> &'static str;

    /// Execute one run. Implementations build a throwaway workspace,
    /// run the suite in isolation, parse results + coverage, and always
    /// clean up the workspace before returning (§10).
    ///
    /// # Errors
    ///
    /// Returns [`RunnerError`] for an unavailable daemon, invalid input,
    /// timeout, cancellation, or any workspace / parse / process failure.
    /// A suite that runs cleanly but has failing assertions is **not** an
    /// error — it returns `Ok` with [`RunStatus::Failed`].
    ///
    /// `cancel` lets the caller stop an in-flight run (UI Stop button); the
    /// runner must kill the container and return [`RunnerError::Cancelled`]
    /// when it fires.
    async fn run(
        &self,
        input: RunInput,
        cancel: CancelToken,
    ) -> Result<RunnerOutput, RunnerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_status_round_trips_through_serde() {
        let cases = [
            (RunStatus::Pending, "pending"),
            (RunStatus::Running, "running"),
            (RunStatus::Passed, "passed"),
            (RunStatus::Failed, "failed"),
            (RunStatus::Error, "error"),
            (RunStatus::Cancelled, "cancelled"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(RunStatus::from_str_value(expected), Some(variant));
            // serde wire form must match the as_str helper exactly.
            let json = serde_json::to_string(&variant).expect("serialize status");
            assert_eq!(json, format!("\"{expected}\""));
        }
        assert_eq!(RunStatus::from_str_value("queued"), None);
    }

    #[test]
    fn test_status_round_trips_through_serde() {
        let cases = [
            (TestStatus::Passed, "passed"),
            (TestStatus::Failed, "failed"),
            (TestStatus::Skipped, "skipped"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(TestStatus::from_str_value(expected), Some(variant));
            let json = serde_json::to_string(&variant).expect("serialize status");
            assert_eq!(json, format!("\"{expected}\""));
        }
        assert_eq!(TestStatus::from_str_value("errored"), None);
    }

    #[test]
    fn run_request_deserializes_camel_case_wire_keys() {
        // clientRunId is optional on the wire (defaults to empty).
        let req: RunRequest =
            serde_json::from_str(r#"{"artifactId":"a1","optInConfirmed":true}"#)
                .expect("deserialize request");
        assert_eq!(req.artifact_id, "a1");
        assert!(req.opt_in_confirmed);
        assert!(req.client_run_id.is_empty());

        let with_id: RunRequest = serde_json::from_str(
            r#"{"artifactId":"a1","optInConfirmed":true,"clientRunId":"run-9"}"#,
        )
        .expect("deserialize request with id");
        assert_eq!(with_id.client_run_id, "run-9");
    }

    #[test]
    fn run_result_serializes_camel_case_and_omits_none_options() {
        let result = RunResult {
            run_id: "r1".into(),
            status: RunStatus::Passed,
            passed_count: 2,
            failed_count: 0,
            duration_ms: 350,
            tests: vec![
                TestResult {
                    name: "adds two numbers".into(),
                    status: TestStatus::Passed,
                    duration_ms: 10,
                    failure_message: None,
                    source_line: None,
                },
                TestResult {
                    name: "throws on bad input".into(),
                    status: TestStatus::Failed,
                    duration_ms: 4,
                    failure_message: Some("expected 2 to equal 3".into()),
                    source_line: Some(42),
                },
            ],
            coverage: vec![CoverageLine {
                file_path: "src/add.ts".into(),
                line: 1,
                hits: 1,
            }],
            error_message: None,
        };

        let value = serde_json::to_value(&result).expect("serialize result");

        // camelCase keys.
        assert!(value.get("runId").is_some());
        assert!(value.get("passedCount").is_some());
        assert!(value.get("durationMs").is_some());
        // None top-level Option is omitted (mirrors Zod `.optional()`).
        assert!(value.get("errorMessage").is_none());

        let passing = &value["tests"][0];
        assert_eq!(passing["status"], "passed");
        assert!(passing.get("failureMessage").is_none());
        assert!(passing.get("sourceLine").is_none());

        let failing = &value["tests"][1];
        assert_eq!(failing["failureMessage"], "expected 2 to equal 3");
        assert_eq!(failing["sourceLine"], 42);

        assert_eq!(value["coverage"][0]["filePath"], "src/add.ts");

        // Round-trips back to an equal struct.
        let back: RunResult = serde_json::from_value(value).expect("deserialize result");
        assert_eq!(back.run_id, "r1");
        assert_eq!(back.tests.len(), 2);
        assert_eq!(back.status, RunStatus::Passed);
    }

    fn test_file(path: &str) -> WorkspaceFile {
        WorkspaceFile {
            relative_path: path.into(),
            contents: "x".into(),
            is_test: true,
        }
    }

    #[test]
    fn run_input_validate_accepts_safe_workspace() {
        let input = RunInput {
            language: RunnerLanguage::TypeScript,
            files: vec![
                test_file("add.test.ts"),
                WorkspaceFile {
                    relative_path: "src/add.ts".into(),
                    contents: "export const add = (a, b) => a + b;".into(),
                    is_test: false,
                },
            ],
            limits: ResourceLimits::default(),
        };
        input.validate().expect("safe workspace");
    }

    #[test]
    fn run_input_validate_rejects_empty_and_testless_workspaces() {
        let empty = RunInput {
            language: RunnerLanguage::JavaScript,
            files: vec![],
            limits: ResourceLimits::default(),
        };
        assert_eq!(empty.validate().unwrap_err().code(), "INVALID_INPUT");

        let no_test = RunInput {
            language: RunnerLanguage::JavaScript,
            files: vec![WorkspaceFile {
                relative_path: "src/add.js".into(),
                contents: "module.exports = {};".into(),
                is_test: false,
            }],
            limits: ResourceLimits::default(),
        };
        assert_eq!(no_test.validate().unwrap_err().code(), "INVALID_INPUT");
    }

    #[test]
    fn run_input_validate_rejects_path_traversal() {
        for bad in [
            "../escape.test.ts",
            "/etc/passwd.test.ts",
            "nested/../../escape.test.ts",
            "C:\\windows\\evil.test.ts",
            "\\absolute.test.ts",
        ] {
            let input = RunInput {
                language: RunnerLanguage::TypeScript,
                files: vec![test_file(bad)],
                limits: ResourceLimits::default(),
            };
            assert_eq!(
                input.validate().unwrap_err().code(),
                "INVALID_INPUT",
                "path `{bad}` must be rejected"
            );
        }
    }

    #[test]
    fn is_safe_relative_path_allows_nested_forward_paths() {
        assert!(is_safe_relative_path("src/lib/add.ts"));
        assert!(is_safe_relative_path("add.test.ts"));
        assert!(!is_safe_relative_path(""));
        assert!(!is_safe_relative_path("   "));
        assert!(!is_safe_relative_path("a/../b"));
    }

    #[test]
    fn runner_language_detects_typescript_extensions() {
        assert_eq!(RunnerLanguage::from_path("a.ts"), RunnerLanguage::TypeScript);
        assert_eq!(RunnerLanguage::from_path("a.TSX"), RunnerLanguage::TypeScript);
        assert_eq!(RunnerLanguage::from_path("a.mts"), RunnerLanguage::TypeScript);
        assert_eq!(RunnerLanguage::from_path("a.js"), RunnerLanguage::JavaScript);
        assert_eq!(RunnerLanguage::from_path("a.jsx"), RunnerLanguage::JavaScript);
    }

    #[test]
    fn runner_error_codes_are_stable() {
        assert_eq!(RunnerError::Cancelled.code(), "RUNNER_CANCELLED");
        assert_eq!(RunnerError::Timeout(60).code(), "RUNNER_TIMEOUT");
        assert_eq!(
            RunnerError::DockerUnavailable("x".into()).code(),
            "DOCKER_UNAVAILABLE"
        );
        assert_eq!(
            RunnerError::ImageMissing("x".into()).code(),
            "RUNNER_IMAGE_MISSING"
        );
    }

    #[test]
    fn run_input_validate_rejects_too_many_files() {
        let files = (0..=MAX_WORKSPACE_FILES)
            .map(|i| WorkspaceFile {
                relative_path: format!("f{i}.test.ts"),
                contents: "x".into(),
                is_test: true,
            })
            .collect();
        let input = RunInput {
            language: RunnerLanguage::TypeScript,
            files,
            limits: ResourceLimits::default(),
        };
        assert_eq!(input.validate().unwrap_err().code(), "INVALID_INPUT");
    }

    #[test]
    fn run_input_validate_rejects_oversize_workspace() {
        let input = RunInput {
            language: RunnerLanguage::TypeScript,
            files: vec![WorkspaceFile {
                relative_path: "big.test.ts".into(),
                contents: "a".repeat(MAX_WORKSPACE_BYTES + 1),
                is_test: true,
            }],
            limits: ResourceLimits::default(),
        };
        assert_eq!(input.validate().unwrap_err().code(), "INVALID_INPUT");
    }

    #[tokio::test]
    async fn cancel_token_resolves_after_cancel() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
        // Already-cancelled token must resolve immediately.
        token.cancelled().await;
    }

    #[tokio::test]
    async fn cancel_token_wakes_a_pending_waiter() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move { waiter.cancelled().await });
        token.cancel();
        // Must complete; a lost wakeup would hang the test.
        handle.await.expect("waiter task joins");
    }

    #[tokio::test]
    async fn fresh_cancel_token_does_not_resolve() {
        let token = CancelToken::new();
        let pending = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            token.cancelled(),
        )
        .await;
        assert!(pending.is_err(), "an un-cancelled token must keep pending");
    }
}
