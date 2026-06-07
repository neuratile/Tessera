//! Sandbox service — the sole entry point for executing a generated
//! test-case artifact in an isolated runner (plan §4).
//!
//! Mirrors `generation_service`'s role for the LLM path: commands depend
//! on this service, the service depends on the [`TestRunner`] trait, and
//! all SQL is delegated to `test_run_repo`. Docker specifics live only in
//! [`docker_js`](crate::providers::runners::docker_js).
//!
//! Flow (plan §4):
//!
//! 1. Enforce the opt-in gate — reject when `optInConfirmed` is false
//!    (defence in depth, plan §3; not just a hidden UI button).
//! 2. Load the artifact, require it be a test-cases artifact.
//! 3. Build a [`RunInput`] (source + generated test files) from the
//!    artifact's `structured_data`.
//! 4. Open a `pending` run row, mark it `running`.
//! 5. Drive the runner; persist cases + coverage; finalize the run.
//! 6. Read the run back as a [`RunResult`] for the renderer.
//!
//! A runner failure (Docker down, timeout, parse error) is **not**
//! propagated as an `Err`: the run is finalized with [`RunStatus::Error`]
//! and returned so the UI can show the failure. Only pre-flight problems
//! (opt-out, missing / wrong-type artifact, unbuildable workspace) short
//! circuit with an `Err`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use serde_json::Value as JsonValue;

use crate::error::{AppError, AppResult};
use crate::providers::runners::{
    CancelToken, RunInput, RunRequest, RunResult, RunStatus, RunnerError, RunnerLanguage,
    RunnerOutput, ResourceLimits, TestRunner, TestStatus, WorkspaceFile,
};
use crate::repositories::artifact_repo::{self, ArtifactType};
use crate::repositories::test_run_repo::{self, RunOutcome, TestRunInsert};

/// In-flight run → [`CancelToken`] map, shared between [`run`] (which
/// registers a token for the duration of a run) and the `cancel_test_sandbox`
/// command (which fires it on a user Stop). Managed as Tauri state so both the
/// run command and the cancel command see the same map (plan §5 — UI Stop
/// wiring). Cloning shares the inner map (`Arc`).
#[derive(Clone, Default)]
pub struct RunRegistry {
    inner: Arc<Mutex<HashMap<String, CancelToken>>>,
}

impl RunRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, run_id: &str, token: CancelToken) {
        self.lock().insert(run_id.to_string(), token);
    }

    fn unregister(&self, run_id: &str) {
        self.lock().remove(run_id);
    }

    /// Fire the cancellation token for `run_id`. Returns `true` when a live
    /// run matched (token fired), `false` when no such run is in flight (it
    /// already finished, or the id is unknown — both are no-ops for the UI).
    #[must_use]
    pub fn cancel(&self, run_id: &str) -> bool {
        let token = self.lock().get(run_id).cloned();
        match token {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CancelToken>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Unregisters a run's token on scope exit so a token never outlives its
/// run, on the happy path or any early `?` return.
struct RegistryGuard<'a> {
    registry: &'a RunRegistry,
    run_id: &'a str,
}

impl Drop for RegistryGuard<'_> {
    fn drop(&mut self) {
        self.registry.unregister(self.run_id);
    }
}

/// References [`run`] needs, bundled so the public signature stays short
/// and the runner is trivially swappable in tests (mirrors
/// `GenerationDeps`).
pub struct SandboxDeps<'a> {
    pub pool: &'a sqlx::SqlitePool,
    pub runner: Arc<dyn TestRunner>,
    /// Live-run registry so a concurrent `cancel_test_sandbox` can stop this
    /// run mid-flight. Tests pass a throwaway [`RunRegistry::new`].
    pub registry: &'a RunRegistry,
}

/// Execute one sandboxed run end to end and return the persisted result.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `optInConfirmed` is false, the
///   `artifactId` is empty, the artifact is not a test-cases artifact, or
///   its `structured_data` carries no runnable test files.
/// - [`AppError::NotFound`] when the artifact does not exist.
/// - [`AppError::Database`] for any SQLx-level failure.
///
/// Runner-level failures do not error here — they are persisted as an
/// [`RunStatus::Error`] run and returned in the [`RunResult`].
pub async fn run(request: RunRequest, deps: &SandboxDeps<'_>) -> AppResult<RunResult> {
    // 1. Opt-in gate (plan §3 — backend rejects when the flag is off).
    if !request.opt_in_confirmed {
        return Err(AppError::InvalidInput(
            "sandbox execution is opt-in; optInConfirmed must be true".into(),
        ));
    }
    if request.artifact_id.trim().is_empty() {
        return Err(AppError::InvalidInput("artifactId is empty".into()));
    }

    // 2. Load + type-check the artifact.
    let artifact = artifact_repo::fetch(deps.pool, &request.artifact_id).await?;
    if artifact.artifact_type != ArtifactType::TestCases {
        return Err(AppError::InvalidInput(format!(
            "artifact {} is a {} artifact; only test-cases artifacts can be run",
            request.artifact_id,
            artifact.artifact_type.as_ipc_str()
        )));
    }

    // 3. Build the workspace contract from the artifact.
    let input = build_run_input(&artifact.structured_data)?;

    // 4. Open the run row.
    let run_id = test_run_repo::insert_run(
        deps.pool,
        TestRunInsert {
            artifact_id: artifact.id.clone(),
            project_id: artifact.project_id.clone(),
            runner: deps.runner.name().to_string(),
        },
    )
    .await?;

    let span = tracing::info_span!(
        "sandbox_run",
        run_id = %run_id,
        runner = deps.runner.name(),
        files = input.files.len(),
    );
    let _enter = span.enter();

    test_run_repo::mark_running(deps.pool, &run_id).await?;

    // 5. Drive the runner. The cancellation token is registered under the
    //    caller's `client_run_id` (known to the UI before this IPC returns)
    //    so a concurrent `cancel_test_sandbox` can fire it; the
    //    `RegistryGuard` removes it on every exit path. A run with no
    //    client id is simply not cancellable. The runner's own wall-clock
    //    timeout is independent of this user-Stop token.
    let cancel = CancelToken::new();
    let cancel_key = request.client_run_id.trim().to_string();
    if !cancel_key.is_empty() {
        deps.registry.register(&cancel_key, cancel.clone());
    }
    let _guard = RegistryGuard { registry: deps.registry, run_id: &cancel_key };

    tracing::debug!("driving runner");
    let started = Instant::now();
    let outcome = deps.runner.run(input, cancel).await;
    let duration_ms = elapsed_ms(started);

    match &outcome {
        Ok(output) => tracing::debug!(status = output.status.as_str(), duration_ms, "runner finished"),
        Err(err) => tracing::debug!(code = err.code(), duration_ms, "runner errored"),
    }

    match outcome {
        Ok(output) => persist_success(deps, &run_id, output, duration_ms).await?,
        Err(err) => persist_failure(deps, &run_id, &err, duration_ms).await?,
    }

    // 6. Read back the canonical result.
    test_run_repo::fetch_run(deps.pool, &run_id).await
}

/// Request cancellation of an in-flight run. Returns `true` when a live run
/// matched. The orchestration entry point for the `cancel_test_sandbox`
/// command (commands depend on the service, not the registry internals).
#[must_use]
pub fn request_cancel(registry: &RunRegistry, run_id: &str) -> bool {
    let cancelled = registry.cancel(run_id);
    tracing::info!(run_id, cancelled, "sandbox cancel requested");
    cancelled
}

/// Persist a completed run's cases, coverage, and terminal status.
async fn persist_success(
    deps: &SandboxDeps<'_>,
    run_id: &str,
    output: RunnerOutput,
    duration_ms: u32,
) -> AppResult<()> {
    // Capture insert errors instead of `?`-returning, so `finalize_run` is
    // always reached. Otherwise a failure between the two inserts (e.g.
    // insert_cases commits but insert_coverage errors) would leave the row
    // stuck in `running` forever, never reaching a terminal status.
    let write_err = async {
        test_run_repo::insert_cases(deps.pool, run_id, &output.tests).await?;
        test_run_repo::insert_coverage(deps.pool, run_id, &output.coverage).await?;
        Ok::<(), AppError>(())
    }
    .await
    .err();

    let passed_count = count_status(&output.tests, TestStatus::Passed);
    let failed_count = count_status(&output.tests, TestStatus::Failed);
    let (status, error_message) = match &write_err {
        Some(e) => (RunStatus::Error, Some(format!("DB write failed: {e}"))),
        None => (output.status, None),
    };

    test_run_repo::finalize_run(
        deps.pool,
        run_id,
        RunOutcome {
            status,
            passed_count,
            failed_count,
            duration_ms,
            error_message,
        },
    )
    .await?;

    write_err.map_or(Ok(()), Err)
}

/// Finalize a run that failed inside the runner with a typed error,
/// surfacing the runner's error code in the message for the UI.
async fn persist_failure(
    deps: &SandboxDeps<'_>,
    run_id: &str,
    err: &RunnerError,
    duration_ms: u32,
) -> AppResult<()> {
    tracing::warn!(run_id, code = err.code(), error = %err, "sandbox run failed");
    let status = match err {
        RunnerError::Cancelled => RunStatus::Cancelled,
        _ => RunStatus::Error,
    };
    test_run_repo::finalize_run(
        deps.pool,
        run_id,
        RunOutcome {
            status,
            passed_count: 0,
            failed_count: 0,
            duration_ms,
            error_message: Some(format!("[{}] {err}", err.code())),
        },
    )
    .await
}

/// Build a [`RunInput`] from the artifact's `structured_data.files`.
///
/// The test-cases artifact carries the runnable workspace under a `files`
/// array — `[{ "path": "...", "contents": "...", "isTest": bool }]`. The
/// language is inferred from the test-file extensions.
///
/// # Errors
///
/// [`AppError::InvalidInput`] when the `files` array is absent, empty,
/// malformed, or fails workspace validation (no test file / unsafe path).
fn build_run_input(structured_data: &JsonValue) -> AppResult<RunInput> {
    let raw_files = structured_data
        .get("files")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            AppError::InvalidInput(
                "artifact has no runnable test files (expected structured_data.files[])".into(),
            )
        })?;

    let mut files = Vec::with_capacity(raw_files.len());
    for (index, raw) in raw_files.iter().enumerate() {
        let path = raw
            .get("path")
            .and_then(JsonValue::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                AppError::InvalidInput(format!("files[{index}] is missing a non-empty `path`"))
            })?;
        let contents = raw
            .get("contents")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                AppError::InvalidInput(format!("files[{index}] is missing string `contents`"))
            })?;
        let is_test = raw
            .get("isTest")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);

        files.push(WorkspaceFile {
            relative_path: path.to_string(),
            contents: contents.to_string(),
            is_test,
        });
    }

    let language = files
        .iter()
        .filter(|f| f.is_test)
        .map(|f| RunnerLanguage::from_path(&f.relative_path))
        .find(|lang| *lang == RunnerLanguage::TypeScript)
        .unwrap_or(RunnerLanguage::JavaScript);

    let input = RunInput {
        language,
        files,
        limits: ResourceLimits::default(),
    };
    input
        .validate()
        .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(input)
}

fn count_status(tests: &[crate::providers::runners::TestResult], status: TestStatus) -> u32 {
    let n = tests.iter().filter(|t| t.status == status).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn elapsed_ms(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::providers::runners::{CoverageLine, TestResult};
    use crate::repositories::artifact_repo::{ArtifactInsert, GenerationMetadata};
    use async_trait::async_trait;
    use chrono::Utc;
    use sqlx::SqlitePool;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// Mock runner that yields a scripted outcome — no Docker required
    /// (mirrors the `ScriptedLlm` pattern in `generation_service`).
    enum Scripted {
        Succeed(RunnerOutput),
        Fail(RunnerError),
    }

    struct ScriptedRunner {
        outcome: std::sync::Mutex<Option<Scripted>>,
    }

    impl ScriptedRunner {
        fn new(outcome: Scripted) -> Self {
            Self {
                outcome: std::sync::Mutex::new(Some(outcome)),
            }
        }
    }

    #[async_trait]
    impl TestRunner for ScriptedRunner {
        fn name(&self) -> &'static str {
            "scripted-runner"
        }
        async fn run(
            &self,
            input: RunInput,
            _cancel: CancelToken,
        ) -> Result<RunnerOutput, RunnerError> {
            // Prove the service always hands the runner a valid workspace.
            input.validate().expect("service must pass a valid RunInput");
            match self
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .expect("ScriptedRunner run called more than once")
            {
                Scripted::Succeed(output) => Ok(output),
                Scripted::Fail(err) => Err(err),
            }
        }
    }

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-sandbox-{}.db", Uuid::new_v4()))
    }

    /// Seed a project + a test-cases artifact whose `structured_data`
    /// carries a runnable `files` array. Returns the artifact id.
    async fn seed_artifact(pool: &SqlitePool, artifact_type: ArtifactType) -> String {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("seed project");

        artifact_repo::insert(
            pool,
            ArtifactInsert {
                project_id: "p1".into(),
                artifact_type,
                title: "Cases v1".into(),
                content_md: "# Cases\n".into(),
                structured_data: serde_json::json!({
                    "files": [
                        { "path": "src/add.ts", "contents": "export const add = (a, b) => a + b;", "isTest": false },
                        { "path": "add.test.ts", "contents": "import { test, expect } from 'vitest';", "isTest": true }
                    ]
                }),
                generation_metadata: GenerationMetadata {
                    provider: "ollama".into(),
                    model: "qwen2.5-coder:7b".into(),
                    prompt_version: "test_cases_v1".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    started_at: now.clone(),
                    completed_at: now.clone(),
                },
                parent_id: None,
            },
        )
        .await
        .expect("seed artifact")
    }

    async fn open_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        (pool, path)
    }

    fn sample_output() -> RunnerOutput {
        RunnerOutput {
            status: RunStatus::Failed,
            tests: vec![
                TestResult {
                    name: "adds two numbers".into(),
                    status: TestStatus::Passed,
                    duration_ms: 10,
                    failure_message: None,
                    source_line: None,
                },
                TestResult {
                    name: "rejects bad input".into(),
                    status: TestStatus::Failed,
                    duration_ms: 4,
                    failure_message: Some("expected 2 to equal 3".into()),
                    source_line: Some(42),
                },
            ],
            coverage: vec![
                CoverageLine { file_path: "src/add.ts".into(), line: 1, hits: 3 },
                CoverageLine { file_path: "src/add.ts".into(), line: 2, hits: 0 },
            ],
            stdout: "ran 2 tests".into(),
            stderr: String::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_persists_results_and_returns_run_result() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool, ArtifactType::TestCases).await;

        let runner: Arc<dyn TestRunner> =
            Arc::new(ScriptedRunner::new(Scripted::Succeed(sample_output())));
        let registry = RunRegistry::new();
        let deps = SandboxDeps { pool: &pool, runner, registry: &registry };

        let result = run(
            RunRequest {
                artifact_id: artifact_id.clone(),
                opt_in_confirmed: true,
                client_run_id: String::new(),
            },
            &deps,
        )
        .await
        .expect("run succeeds");

        assert_eq!(result.status, RunStatus::Failed);
        assert_eq!(result.passed_count, 1);
        assert_eq!(result.failed_count, 1);
        assert_eq!(result.tests.len(), 2);
        assert_eq!(result.coverage.len(), 2);
        assert_eq!(result.coverage[1].hits, 0);
        assert!(result.error_message.is_none());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_rejects_when_opt_in_not_confirmed() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool, ArtifactType::TestCases).await;

        let runner: Arc<dyn TestRunner> =
            Arc::new(ScriptedRunner::new(Scripted::Succeed(sample_output())));
        let registry = RunRegistry::new();
        let deps = SandboxDeps { pool: &pool, runner, registry: &registry };

        let err = run(
            RunRequest {
                artifact_id,
                opt_in_confirmed: false,
                client_run_id: String::new(),
            },
            &deps,
        )
        .await
        .expect_err("must reject opt-out");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_rejects_non_test_cases_artifact() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool, ArtifactType::TestPlan).await;

        let runner: Arc<dyn TestRunner> =
            Arc::new(ScriptedRunner::new(Scripted::Succeed(sample_output())));
        let registry = RunRegistry::new();
        let deps = SandboxDeps { pool: &pool, runner, registry: &registry };

        let err = run(
            RunRequest {
                artifact_id,
                opt_in_confirmed: true,
                client_run_id: String::new(),
            },
            &deps,
        )
        .await
        .expect_err("must reject wrong type");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_persists_error_run_when_runner_fails() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool, ArtifactType::TestCases).await;

        let runner: Arc<dyn TestRunner> = Arc::new(ScriptedRunner::new(Scripted::Fail(
            RunnerError::DockerUnavailable("daemon down".into()),
        )));
        let registry = RunRegistry::new();
        let deps = SandboxDeps { pool: &pool, runner, registry: &registry };

        let result = run(
            RunRequest {
                artifact_id,
                opt_in_confirmed: true,
                client_run_id: String::new(),
            },
            &deps,
        )
        .await
        .expect("runner failure is persisted, not propagated");

        assert_eq!(result.status, RunStatus::Error);
        assert_eq!(result.passed_count, 0);
        assert_eq!(result.failed_count, 0);
        let message = result.error_message.expect("error message present");
        assert!(message.contains("DOCKER_UNAVAILABLE"), "got: {message}");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_persists_error_run_when_runner_image_is_missing() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool, ArtifactType::TestCases).await;

        let runner: Arc<dyn TestRunner> = Arc::new(ScriptedRunner::new(Scripted::Fail(
            RunnerError::ImageMissing("runner image `tessera-runner-js` is not built".into()),
        )));
        let registry = RunRegistry::new();
        let deps = SandboxDeps { pool: &pool, runner, registry: &registry };

        let result = run(
            RunRequest {
                artifact_id,
                opt_in_confirmed: true,
                client_run_id: String::new(),
            },
            &deps,
        )
        .await
        .expect("runner failure is persisted, not propagated");

        assert_eq!(result.status, RunStatus::Error);
        let message = result.error_message.expect("error message present");
        assert!(message.contains("RUNNER_IMAGE_MISSING"), "got: {message}");
        assert!(message.contains("not built"), "got: {message}");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_rejects_artifact_without_files() {
        let (pool, path) = open_pool().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed project");

        // A descriptive test-cases artifact with no runnable `files` array.
        let artifact_id = artifact_repo::insert(
            &pool,
            ArtifactInsert {
                project_id: "p1".into(),
                artifact_type: ArtifactType::TestCases,
                title: "Cases".into(),
                content_md: "# Cases\n".into(),
                structured_data: serde_json::json!({ "cases": [] }),
                generation_metadata: GenerationMetadata {
                    provider: "ollama".into(),
                    model: "m".into(),
                    prompt_version: "test_cases_v1".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    started_at: now.clone(),
                    completed_at: now.clone(),
                },
                parent_id: None,
            },
        )
        .await
        .expect("seed artifact");

        let runner: Arc<dyn TestRunner> =
            Arc::new(ScriptedRunner::new(Scripted::Succeed(sample_output())));
        let registry = RunRegistry::new();
        let deps = SandboxDeps { pool: &pool, runner, registry: &registry };

        let err = run(
            RunRequest {
                artifact_id,
                opt_in_confirmed: true,
                client_run_id: String::new(),
            },
            &deps,
        )
        .await
        .expect_err("must reject artifact without files");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn registry_cancel_fires_only_the_matching_live_run() {
        let registry = RunRegistry::new();
        let token = CancelToken::new();
        registry.register("run-1", token.clone());

        // Unknown id is a no-op; the live run's token is untouched.
        assert!(!request_cancel(&registry, "run-x"));
        assert!(!token.is_cancelled());

        // Matching id fires the token.
        assert!(request_cancel(&registry, "run-1"));
        assert!(token.is_cancelled());

        // After the run deregisters, a late Stop is a no-op.
        registry.unregister("run-1");
        assert!(!request_cancel(&registry, "run-1"));
    }

    /// Runner that blocks until its cancel token fires, then reports the
    /// run cancelled — models a long Docker run a user Stops.
    struct BlockingRunner;

    #[async_trait]
    impl TestRunner for BlockingRunner {
        fn name(&self) -> &'static str {
            "blocking-runner"
        }
        async fn run(
            &self,
            input: RunInput,
            cancel: CancelToken,
        ) -> Result<RunnerOutput, RunnerError> {
            input.validate().expect("service must pass a valid RunInput");
            cancel.cancelled().await;
            Err(RunnerError::Cancelled)
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_is_cancellable_mid_flight_via_client_run_id() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed_artifact(&pool, ArtifactType::TestCases).await;
        let registry = RunRegistry::new();
        let runner: Arc<dyn TestRunner> = Arc::new(BlockingRunner);

        // Drive the (blocking) run on a task; it shares the registry so a
        // concurrent Stop reaches the same cancel token.
        let reg = registry.clone();
        let pool_for_run = pool.clone();
        let aid = artifact_id.clone();
        let handle = tokio::spawn(async move {
            let deps = SandboxDeps { pool: &pool_for_run, runner, registry: &reg };
            run(
                RunRequest {
                    artifact_id: aid,
                    opt_in_confirmed: true,
                    client_run_id: "client-xyz".into(),
                },
                &deps,
            )
            .await
        });

        // Spin until the run has registered its token, then Stop it.
        loop {
            if request_cancel(&registry, "client-xyz") {
                break;
            }
            tokio::task::yield_now().await;
        }

        let result = handle.await.expect("join").expect("run returns a result");
        assert_eq!(result.status, RunStatus::Cancelled);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
