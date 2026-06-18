//! Mutation-testing orchestrator — Stage 1 (score)
//! (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md`).
//!
//! Mutation testing measures what line coverage cannot: *would the suite fail
//! if the code were wrong?* [`score`] runs the suite once for a green baseline,
//! mutates each **covered** source line with a single small edit, reruns the
//! unchanged suite per mutant, and reports a kill/survive **mutation score**.
//!
//! Like [`healing_service`](crate::services::healing_service), this is pure
//! composition over existing entry points and owns *no* new prompt or output
//! schema. It reuses [`sandbox_service`] verbatim:
//!
//! - [`sandbox_service::run`] — the persisted, opt-in-gated baseline run (its
//!   coverage gates which lines are worth mutating; its `run_id` is the
//!   recorded baseline).
//! - [`sandbox_service::prepare_run`] — the shared preamble yielding the source
//!   and test [`RunInput`] plus the matching runner, which this service drives
//!   directly against each mutated workspace (a mutant is not an artifact, so
//!   it cannot go back through `run`).
//!
//! The new domain logic is only the mutant engine
//! ([`crate::providers::runners::mutation`]); everything else is glue. A
//! sibling-service rule (`rules.md` §4.2) forbids `sandbox_service` from calling
//! `generation_service` (Stage 2) and vice-versa, so the loop that will need
//! both lives here — exactly how `healing_service` was justified.
//!
//! Performance is the dominant constraint — this is inherently N+1 runs (design
//! §4). Three defenses, all here: coverage-guided selection (only covered lines
//! are mutated — the biggest win), a mutant cap with a logged drop count (no
//! silent truncation), and first-failure kill semantics inherited from the
//! runner (a mutant is killed the instant one test fails).

use std::collections::{HashMap, HashSet};

use crate::error::{AppError, AppResult};
use crate::providers::runners::mutation::{
    apply_mutant, cap_mutants, generate_mutants, MutantResult, MutantStatus, MutationCheckRecord,
    MutationCheckSummary, MutationResult,
};
use crate::providers::runners::{RunInput, RunRequest, RunStatus, RunnerError, WorkspaceFile};
use crate::repositories::mutation_check_repo::{self, MutationCheckInsert};
use crate::services::sandbox_service::{self, SandboxDeps};

/// Progress event delivered to the optional [`MutationSink`] after each mutant
/// run, so the UI can stream "mutant 12 / 40".
#[derive(Debug, Clone, Copy)]
pub enum MutationEvent {
    Mutant { done: u32, total: u32 },
}

/// Per-event hook the caller can supply to relay sweep progress to the UI.
/// Forwarding is best-effort — the loop continues even if the closure errors.
pub type MutationSink = Box<dyn FnMut(MutationEvent) + Send>;

/// Run the bounded mutation-score sweep over `request.artifact_id` (design §2).
///
/// `max_mutants` is re-clamped to `[MUT_MIN_MUTANTS, MUT_MAX_MUTANTS]` by the
/// engine's [`cap_mutants`]. The whole sweep shares one cancel token registered
/// under `request.client_run_id`, so the existing `cancel_test_sandbox` Stop
/// kills the in-flight container and ends the sweep.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] for any pre-flight problem `run` itself rejects
///   (opt-out, missing / wrong-type artifact, no runnable files), **and** when
///   the baseline suite is not all-green — mutation scoring against a red suite
///   is meaningless (design §2, the one rule that makes the score trustworthy).
/// - [`AppError::Internal`] when the sweep is cancelled or the runner becomes
///   unavailable mid-sweep (the score has no partial form to return — unlike a
///   per-mutant build failure, which is just an excluded "errored" mutant).
/// - [`AppError::Database`] for any SQLx-level failure in the baseline run.
pub async fn score(
    request: RunRequest,
    max_mutants: u32,
    deps: &SandboxDeps<'_>,
    mut on_event: Option<MutationSink>,
) -> AppResult<MutationResult> {
    // Register ONE cancel token under the caller's `client_run_id` spanning the
    // *entire* operation — the baseline run and every per-mutant run. Without
    // this, `run` would register-then-deregister its own token and the sweep's
    // token would only be registered after `prepare_run` + `cap_mutants`,
    // leaving a window where a user Stop finds no token and is silently dropped
    // (Greptile review). Holding it here closes that gap.
    let (cancel, _guard) = sandbox_service::register_cancel(deps, &request.client_run_id);

    // 1. Baseline: run the suite once through the existing persisted, opt-in
    //    gated path, under the shared token. `post_jira_comment = false`: this
    //    baseline is internal to the mutation sweep, not a user-requested run,
    //    so posting its pass/fail to a tracker would be spurious (mirrors how
    //    `run_flaky` suppresses the Jira post for its iteration #1). A pre-flight
    //    `Err` (opt-out, bad artifact) propagates.
    let baseline =
        sandbox_service::run_with_token(request.clone(), deps, cancel.clone(), false).await?;
    if baseline.status == RunStatus::Cancelled {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Mutation test cancelled during the baseline run."
        )));
    }
    gate_green(&baseline)?;

    let span = tracing::info_span!(
        "mutation_score",
        artifact_id = %request.artifact_id,
        baseline_run_id = %baseline.run_id,
    );
    let _enter = span.enter();

    // 2. Re-derive the source + test workspace and the matching runner. This
    //    reloads the artifact (cheap) so the per-mutant runs can drive the
    //    runner directly against a mutated workspace — a mutant is not an
    //    artifact and cannot go back through `run`.
    let prepared = sandbox_service::prepare_run(&request, deps).await?;
    let language = prepared.input.language;

    // 3. Coverage-guided selection (design §4): only lines the baseline actually
    //    executed are worth mutating — an uncovered line is a guaranteed
    //    survivor, so scoring it is pure noise.
    let covered = covered_lines_by_file(&baseline.coverage);

    // 4. Generate one mutant per applicable operator site on a covered line,
    //    across every *source* file (tests are never mutated).
    let empty: HashSet<u32> = HashSet::new();
    let mut all_mutants = Vec::new();
    for file in prepared.input.files.iter().filter(|f| !f.is_test) {
        let lines = covered.get(&file.relative_path).unwrap_or(&empty);
        all_mutants.extend(generate_mutants(&file.relative_path, &file.contents, language, lines));
    }
    let (mutants, dropped_count) = cap_mutants(all_mutants, max_mutants);
    let total = u32::try_from(mutants.len()).unwrap_or(u32::MAX);
    if dropped_count > 0 {
        tracing::info!(kept = total, dropped = dropped_count, "mutant cap applied; some mutants sampled out");
    }

    // A Stop fired during the baseline run or this setup (gate, prepare, mutant
    // generation) already fired the shared token — honour it before spending a
    // single mutant run rather than discovering it on the first `runner.run`.
    if cancel.is_cancelled() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "Mutation test cancelled before the sweep started."
        )));
    }

    // 5. Run the unchanged suite once per mutant under the shared token (a Stop
    //    kills the current container and ends the check, design §4).
    let mut results: Vec<MutantResult> = Vec::with_capacity(mutants.len());
    let (mut killed, mut survived, mut errored) = (0u32, 0u32, 0u32);
    for (index, mutant) in mutants.into_iter().enumerate() {
        let mutated = apply_to_input(&prepared.input, &mutant);
        let status = match prepared.runner.run(mutated, cancel.clone()).await {
            // suite Failed → the bug was caught → killed; Passed → survived;
            // anything else (an Error status) → excluded.
            Ok(output) => match output.status {
                RunStatus::Failed => MutantStatus::Killed,
                RunStatus::Passed => MutantStatus::Survived,
                _ => MutantStatus::Errored,
            },
            // A cancel or a dead daemon aborts the whole sweep — the score has
            // no meaningful partial form.
            Err(RunnerError::Cancelled) => {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "Mutation test cancelled after {index} of {total} mutants."
                )));
            }
            Err(err @ (RunnerError::DockerUnavailable(_) | RunnerError::ImageMissing(_))) => {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "The mutation sweep stopped: [{}] {err}",
                    err.code()
                )));
            }
            // A mutant that won't build / times out / crashes is *not* evidence
            // about the suite — it leaves the score denominator (design §4).
            Err(_) => MutantStatus::Errored,
        };
        match status {
            MutantStatus::Killed => killed += 1,
            MutantStatus::Survived => survived += 1,
            MutantStatus::Errored => errored += 1,
        }
        results.push(MutantResult { mutant, status });
        emit(&mut on_event, u32::try_from(index + 1).unwrap_or(u32::MAX), total);
    }

    let score = mutation_score(killed, survived);
    let total_run = killed + survived + errored;

    // 7. Persist the check as history (design §5.5). Best-effort: a history
    //    write failure is logged and swallowed, never discarding the in-memory
    //    result the user is about to see (the same rule flaky follows).
    if let Err(e) = mutation_check_repo::insert_check(
        deps.pool,
        MutationCheckInsert {
            artifact_id: prepared.artifact.id.clone(),
            project_id: prepared.artifact.project_id.clone(),
            baseline_run_id: Some(baseline.run_id.clone()),
            score,
            killed,
            survived,
            errored,
            total: total_run,
            dropped_count,
        },
        &results,
    )
    .await
    {
        tracing::warn!(baseline_run_id = %baseline.run_id, error = %e, "persisting mutation-check history failed");
    }

    tracing::info!(killed, survived, errored, score, "mutation score complete");

    Ok(MutationResult {
        score,
        killed,
        survived,
        errored,
        total: total_run,
        baseline_run_id: baseline.run_id,
        mutants: results,
        dropped_count,
    })
}

/// List an artifact's persisted mutation-score history, newest first
/// (design §5.5). Thin pass-through so commands depend on the service, not the
/// repository (rules §4.2). `limit` is re-clamped by the repository.
///
/// # Errors
///
/// [`AppError::Database`] for any SQLx-level failure.
pub async fn list_mutation_history(
    pool: &sqlx::SqlitePool,
    artifact_id: &str,
    limit: u32,
) -> AppResult<Vec<MutationCheckSummary>> {
    mutation_check_repo::list_checks(pool, artifact_id, limit).await
}

/// Fetch one persisted mutation check with its per-mutant verdicts (design §5.5).
///
/// # Errors
///
/// - [`AppError::NotFound`] when no check matches `check_id`.
/// - [`AppError::Database`] for a corrupt status string or any `SQLx` failure.
pub async fn get_mutation_check(
    pool: &sqlx::SqlitePool,
    check_id: &str,
) -> AppResult<MutationCheckRecord> {
    mutation_check_repo::fetch_check(pool, check_id).await
}

/// Refuse a non-green baseline up front (design §2) — a survivor cannot be told
/// from a pre-existing failure, so scoring against a red suite is meaningless.
fn gate_green(baseline: &crate::providers::runners::RunResult) -> AppResult<()> {
    match baseline.status {
        RunStatus::Passed => Ok(()),
        RunStatus::Failed => Err(AppError::InvalidInput(format!(
            "mutation scoring needs an all-green baseline; the suite has {} failing test(s). \
             Fix or self-heal them first.",
            baseline.failed_count
        ))),
        other => {
            let detail = baseline
                .error_message
                .clone()
                .unwrap_or_else(|| format!("the baseline run did not complete ({})", other.as_str()));
            Err(AppError::InvalidInput(format!(
                "mutation scoring needs a green baseline; {detail}"
            )))
        }
    }
}

/// Group the covered lines (`hits > 0`) of a baseline run by source file.
fn covered_lines_by_file(
    coverage: &[crate::providers::runners::CoverageLine],
) -> HashMap<String, HashSet<u32>> {
    let mut covered: HashMap<String, HashSet<u32>> = HashMap::new();
    for line in coverage {
        if line.hits > 0 {
            covered.entry(line.file_path.clone()).or_default().insert(line.line);
        }
    }
    covered
}

/// Clone `input`, splicing the mutant into the one source file it edits. The
/// test files (and other sources) pass through unchanged, so the workspace
/// stays valid (it still carries a test file).
fn apply_to_input(input: &RunInput, mutant: &crate::providers::runners::mutation::Mutant) -> RunInput {
    let files = input
        .files
        .iter()
        .map(|f| {
            if !f.is_test && f.relative_path == mutant.file {
                WorkspaceFile {
                    relative_path: f.relative_path.clone(),
                    contents: apply_mutant(&f.contents, mutant),
                    is_test: false,
                }
            } else {
                f.clone()
            }
        })
        .collect();
    RunInput { language: input.language, files, limits: input.limits }
}

/// `killed / (killed + survived)`; `0.0` when nothing was scorable (design §4 —
/// errored mutants leave the denominator). The caller distinguishes the
/// no-mutant case via `total`.
fn mutation_score(killed: u32, survived: u32) -> f64 {
    let denom = killed + survived;
    if denom == 0 {
        0.0
    } else {
        f64::from(killed) / f64::from(denom)
    }
}

fn emit(sink: &mut Option<MutationSink>, done: u32, total: u32) {
    if let Some(sink) = sink.as_mut() {
        sink(MutationEvent::Mutant { done, total });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::providers::runners::{
        CancelToken, CoverageLine, RunnerError, RunnerLanguage, RunnerOutput, TestResult,
        TestRunner, TestStatus,
    };
    use crate::repositories::artifact_repo::{self, ArtifactInsert, ArtifactType, GenerationMetadata};
    use crate::services::sandbox_service::RunRegistry;
    use async_trait::async_trait;
    use chrono::Utc;
    use sqlx::SqlitePool;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, PoisonError};
    use uuid::Uuid;

    /// Runner that yields one queued outcome per `run()` — the sweep runs the
    /// suite once for the baseline plus once per mutant. Mirrors the
    /// `MultiScriptedRunner` in the flaky / heal tests.
    enum Scripted {
        Succeed(RunnerOutput),
        Fail(RunnerError),
    }

    struct MultiScriptedRunner {
        outcomes: Mutex<VecDeque<Scripted>>,
    }

    impl MultiScriptedRunner {
        fn new(outcomes: Vec<Scripted>) -> Self {
            Self { outcomes: Mutex::new(outcomes.into_iter().collect()) }
        }
    }

    #[async_trait]
    impl TestRunner for MultiScriptedRunner {
        fn name(&self) -> &'static str {
            "multi-scripted-runner"
        }
        async fn run(&self, input: RunInput, _cancel: CancelToken) -> Result<RunnerOutput, RunnerError> {
            input.validate().expect("service must pass a valid RunInput");
            match self
                .outcomes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
                .expect("MultiScriptedRunner run called more times than scripted")
            {
                Scripted::Succeed(output) => Ok(output),
                Scripted::Fail(err) => Err(err),
            }
        }
    }

    fn fixed_factory(
        runner: Arc<dyn TestRunner>,
    ) -> impl Fn(RunnerLanguage) -> Arc<dyn TestRunner> + Send + Sync {
        move |_| runner.clone()
    }

    /// Runner that fires the cancel token it is handed *during* the baseline run
    /// (simulating a user Stop landing right as the baseline finishes) and
    /// returns a green baseline. It asserts it is never called a second time —
    /// proving the orchestrator honours the Stop before spending a mutant run,
    /// and that the baseline and the sweep share one token (the gap fix).
    struct CancelDuringBaselineRunner {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl TestRunner for CancelDuringBaselineRunner {
        fn name(&self) -> &'static str {
            "cancel-during-baseline"
        }
        async fn run(&self, input: RunInput, cancel: CancelToken) -> Result<RunnerOutput, RunnerError> {
            input.validate().expect("service must pass a valid RunInput");
            let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
            *calls += 1;
            assert_eq!(*calls, 1, "no mutant must run after a Stop during the baseline");
            // A Stop arrives during the baseline run, firing the shared token.
            cancel.cancel();
            Ok(pass_baseline("src/calc.ts", &[1]))
        }
    }

    fn out(status: RunStatus, tests: Vec<TestResult>, coverage: Vec<CoverageLine>) -> RunnerOutput {
        RunnerOutput { status, tests, coverage, stdout: String::new(), stderr: String::new() }
    }

    fn tc(name: &str, status: TestStatus) -> TestResult {
        TestResult { name: name.into(), status, duration_ms: 1, failure_message: None, source_line: None }
    }

    fn pass_baseline(source_file: &str, lines: &[u32]) -> RunnerOutput {
        let coverage = lines
            .iter()
            .map(|l| CoverageLine { file_path: source_file.into(), line: *l, hits: 1 })
            .collect();
        out(RunStatus::Passed, vec![tc("TC-1 ok", TestStatus::Passed)], coverage)
    }

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-mut-svc-{}.db", Uuid::new_v4()))
    }

    /// Seed a project + a runnable test-cases artifact whose source carries the
    /// given `source_contents`. Returns the artifact id.
    async fn seed(pool: &SqlitePool, source_contents: &str) -> String {
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
                artifact_type: ArtifactType::TestCases,
                title: "Cases v1".into(),
                content_md: "# Cases\n".into(),
                structured_data: serde_json::json!({
                    "files": [
                        { "path": "src/calc.ts", "contents": source_contents, "isTest": false },
                        { "path": "calc.test.ts", "contents": "import { test, expect } from 'vitest';", "isTest": true }
                    ]
                }),
                generation_metadata: GenerationMetadata {
                    provider: "ollama".into(),
                    model: "qwen2.5-coder:7b".into(),
                    prompt_version: "test_cases_v2".into(),
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

    fn request(artifact_id: &str) -> RunRequest {
        RunRequest {
            artifact_id: artifact_id.into(),
            opt_in_confirmed: true,
            client_run_id: String::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn all_mutants_killed_scores_100() {
        let (pool, path) = open_pool().await;
        // `a + b` → one arithmetic mutant on line 1.
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            // The lone mutant is killed (suite fails against it).
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let result = score(request(&artifact_id), 40, &deps, None).await.expect("score runs");
        assert_eq!(result.total, 1);
        assert_eq!(result.killed, 1);
        assert_eq!(result.survived, 0);
        assert!((result.score - 1.0).abs() < f64::EPSILON);
        assert!(!result.baseline_run_id.is_empty());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mixed_kill_and_survive_scores_half() {
        let (pool, path) = open_pool().await;
        // `a + b > 0` → two mutants (relational `>`, arithmetic `+`) on line 1.
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b > 0;").await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])), // killed
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("TC-1 ok", TestStatus::Passed)], vec![])), // survived
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let result = score(request(&artifact_id), 40, &deps, None).await.expect("score runs");
        assert_eq!(result.total, 2);
        assert_eq!(result.killed, 1);
        assert_eq!(result.survived, 1);
        assert!((result.score - 0.5).abs() < f64::EPSILON);

        // Persisted to history.
        let history = list_mutation_history(&pool, &artifact_id, 20).await.expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].total, 2);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn errored_mutant_leaves_the_denominator() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b > 0;").await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])), // killed
            // Second mutant fails to build → errored, excluded from the score.
            Scripted::Fail(RunnerError::Parse("mutated source did not compile".into())),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let result = score(request(&artifact_id), 40, &deps, None).await.expect("score runs");
        assert_eq!(result.killed, 1);
        assert_eq!(result.survived, 0);
        assert_eq!(result.errored, 1);
        assert_eq!(result.total, 2);
        // 1 / (1 + 0) = 100% — the errored mutant is not in the denominator.
        assert!((result.score - 1.0).abs() < f64::EPSILON);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn non_green_baseline_aborts() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;

        // Baseline has a failing test → scoring is refused up front.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let err = score(request(&artifact_id), 40, &deps, None).await.expect_err("must refuse red baseline");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn uncovered_source_yields_no_mutants() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;

        // Baseline passes but reports coverage on a *different* line, so the
        // operator on line 1 is never mutated → zero mutants, only the baseline
        // run is consumed.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[99])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let result = score(request(&artifact_id), 40, &deps, None).await.expect("score runs");
        assert_eq!(result.total, 0);
        assert_eq!(result.mutants.len(), 0);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn streams_one_event_per_mutant() {
        let (pool, path) = open_pool().await;
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b > 0;").await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])),
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("TC-1 ok", TestStatus::Passed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let events = Arc::new(Mutex::new(Vec::<(u32, u32)>::new()));
        let sink_events = events.clone();
        let sink: MutationSink = Box::new(move |event| match event {
            MutationEvent::Mutant { done, total } => {
                sink_events.lock().unwrap_or_else(PoisonError::into_inner).push((done, total));
            }
        });

        let result = score(request(&artifact_id), 40, &deps, Some(sink)).await.expect("score runs");
        assert_eq!(result.total, 2);
        let captured = events.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(captured, vec![(1, 2), (2, 2)]);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_during_baseline_aborts_before_any_mutant_runs() {
        let (pool, path) = open_pool().await;
        // Source has one covered arithmetic operator → a mutant *would* run if
        // the Stop were dropped (the pre-fix bug).
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;

        let runner: Arc<dyn TestRunner> = Arc::new(CancelDuringBaselineRunner { calls: Mutex::new(0) });
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        // The single shared token (registered for the whole `score`) is fired
        // during the baseline; the sweep must abort instead of running mutants.
        let err = score(request(&artifact_id), 40, &deps, None)
            .await
            .expect_err("a Stop during the baseline aborts the sweep");
        assert_eq!(err.code(), "INTERNAL_ERROR");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
