//! Self-healing service — the orchestrator for the bounded
//! run → diagnose → regenerate → rerun loop
//! (`plan/versions/v2/v2-feature-docs/SELF_HEALING_LOOP.md`).
//!
//! This service owns *no* new LLM prompt, output schema, or migration. It is
//! pure composition over the two existing entry points:
//!
//! - [`sandbox_service::run`] — execute the current test-cases artifact in the
//!   hardened, opt-in Docker sandbox (which already enforces the opt-in gate
//!   and registers a cancel token under `client_run_id`).
//! - [`generation_service::generate`] — regenerate the artifact from feedback,
//!   chaining versions via `parent_id`.
//!
//! It mirrors how `run_flaky` owns its loop inside one service: neither
//! `generation_service` nor `sandbox_service` may call the other (layering,
//! rules §4.2), so the loop that needs *both* lives here and holds both
//! dependency bundles.
//!
//! Flow (design §2):
//!
//! 1. Run the current artifact in the sandbox.
//! 2. All tests pass → [`HealOutcome::Healed`].
//! 3. Otherwise synthesize the per-test failures into one `reviewer_feedback`
//!    string, regenerate the whole artifact (with `parent_id` = the current
//!    artifact → a new version), and rerun.
//! 4. Stop when all green, retries are exhausted, the failing-test set stops
//!    shrinking (no progress), or a run/regeneration errors.
//!
//! A runner-level failure (Docker down) or a cancellation is surfaced as a
//! [`HealOutcome::Error`] result carrying an `error_message`, never an `Err`
//! — exactly as `run_flaky` does. An `Err` is reserved for a pre-flight
//! problem on the very first run (e.g. the artifact id is empty).

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::providers::runners::{RunRequest, RunResult, RunStatus, TestStatus};
use crate::repositories::artifact_repo::ArtifactType;
use crate::services::generation_service::{self, GenerationDeps, GenerationRequest};
use crate::services::sandbox_service::{self, SandboxDeps};

/// Lower / upper bounds on the number of attempts a heal runs (design §4).
/// The UI value is only a hint — the backend re-clamps so a tampered IPC
/// payload cannot force an unbounded regenerate loop (mirrors the flaky /
/// opt-in philosophy).
pub const HEAL_MIN_ATTEMPTS: u32 = 1;
pub const HEAL_MAX_ATTEMPTS: u32 = 5;
/// Default attempt budget when the caller supplies none (design §4).
pub const HEAL_DEFAULT_ATTEMPTS: u32 = 3;

/// Cap on how many failing tests are folded into one feedback block, and the
/// per-message length, so the synthesized `reviewer_feedback` stays well
/// within `generate`'s hard token budget (design §5.2).
const MAX_FEEDBACK_FAILURES: usize = 20;
const MAX_FAILURE_MESSAGE_CHARS: usize = 500;

/// Terminal state of a heal loop (design §5.1). `snake_case` wire form mirrors
/// the sibling status enums and the Zod literals in `heal.schema.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealOutcome {
    /// Every test passed on the final attempt — the artifact heals.
    Healed,
    /// The attempt budget ran out before all tests passed; the best attempt
    /// (most passing) is returned.
    Exhausted,
    /// The failing-test set stopped changing between attempts — the model is
    /// stuck, so the loop bailed early rather than burn more LLM calls.
    NoProgress,
    /// A run failed inside the sandbox (Docker down / cancelled) or a
    /// regeneration errored; `error_message` carries the detail.
    Error,
}

impl HealOutcome {
    /// Stable string used in IPC payloads. Matches the serde `snake_case`
    /// wire form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healed => "healed",
            Self::Exhausted => "exhausted",
            Self::NoProgress => "no_progress",
            Self::Error => "error",
        }
    }

    /// Inverse of [`as_str`](Self::as_str). Returns `None` for any
    /// unrecognised string.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "healed" => Some(Self::Healed),
            "exhausted" => Some(Self::Exhausted),
            "no_progress" => Some(Self::NoProgress),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// One failing test carried forward into the next attempt's feedback (and
/// shown in the "attempt N: …" UI trail). Mirrors `HealFailureSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealFailure {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_message: Option<String>,
}

/// Record of one run → (maybe) regenerate cycle. Mirrors `HealAttemptSchema`.
/// `artifact_id` is the version that was *run* on this attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealAttempt {
    pub attempt: u32,
    pub artifact_id: String,
    pub passed_count: u32,
    pub failed_count: u32,
    pub failures: Vec<HealFailure>,
}

/// Aggregate result of a heal loop. Mirrors `HealResultSchema`.
/// `final_artifact_id` / `final_run_id` point at the version the user lands on
/// (the healed attempt, or the best attempt by pass count). `error_message` is
/// set only when `outcome == Error`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealResult {
    pub outcome: HealOutcome,
    pub attempts_used: u32,
    pub final_artifact_id: String,
    pub final_run_id: String,
    pub passed_count: u32,
    pub failed_count: u32,
    pub attempts: Vec<HealAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Inputs to [`heal`]. The regeneration params (`model`, `project_*`,
/// `scope_hint`, `project_summary`) are exactly what `generate` needs to
/// reproduce the artifact; the provider/LLM itself is resolved by the command
/// into [`GenerationDeps`] before this is built.
#[derive(Debug, Clone)]
pub struct HealRequest {
    pub artifact_id: String,
    pub max_attempts: u32,
    pub opt_in_confirmed: bool,
    pub client_run_id: String,
    pub model: String,
    pub project_id: String,
    pub project_name: String,
    pub scope_hint: String,
    pub project_summary: String,
}

/// Progress event delivered to the optional [`HealSink`] after each attempt's
/// run, so the UI can stream "Attempt 2 of 3 · 1 test still failing".
#[derive(Debug, Clone, Copy)]
pub enum HealEvent {
    Attempt { attempt: u32, passed: u32, failed: u32 },
}

/// Per-event hook the caller can supply to relay attempt progress to the UI.
/// Forwarding is best-effort — the loop continues even if the closure errors.
pub type HealSink = Box<dyn FnMut(HealEvent) + Send>;

/// Synthesize the failing tests into one instructive `reviewer_feedback`
/// block for the next regeneration (design §5.2). **Pure** — the
/// unit-testable core of the feedback path.
///
/// The number of failures folded in is capped at [`MAX_FEEDBACK_FAILURES`]
/// and each message at [`MAX_FAILURE_MESSAGE_CHARS`] chars so the result stays
/// within `generate`'s token budget; an overflow is summarized as a trailing
/// "…and N more failing tests" line.
#[must_use]
pub fn synth_feedback(failures: &[HealFailure]) -> String {
    let mut out = String::from(
        "The following generated test cases failed when executed in the sandbox. \
         Fix each failing test so it passes against the source under test — correct \
         wrong expected values, bad imports, and faulty assertions. Do not delete or \
         weaken a test to make it pass; if a test reveals a genuine bug in the source, \
         keep the assertion correct.",
    );

    for failure in failures.iter().take(MAX_FEEDBACK_FAILURES) {
        let message = match failure.failure_message.as_deref() {
            Some(m) if !m.trim().is_empty() => truncate_chars(m.trim(), MAX_FAILURE_MESSAGE_CHARS),
            _ => "(no failure message captured)".to_string(),
        };
        out.push_str("\n- ");
        out.push_str(failure.name.trim());
        out.push_str(": ");
        out.push_str(&message);
    }

    if failures.len() > MAX_FEEDBACK_FAILURES {
        let extra = failures.len() - MAX_FEEDBACK_FAILURES;
        out.push_str("\n- …and ");
        out.push_str(&extra.to_string());
        out.push_str(" more failing tests.");
    }

    out
}

/// Truncate `s` to at most `max` chars on a UTF-8 boundary, appending an
/// ellipsis when it was cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut truncated: String = s.chars().take(max).collect();
    truncated.push('…');
    truncated
}

/// Run the bounded self-healing loop over `request.artifact_id` (design §2, §4).
///
/// Reuses [`sandbox_service::run`] and [`generation_service::generate`]
/// verbatim — every iteration runs through the sandbox's existing opt-in gate
/// and per-run cancel-token registration, so the existing `cancel_test_sandbox`
/// Stop kills the in-flight container and the loop observes the cancelled run.
///
/// # Errors
///
/// Returns `Err` only for a pre-flight problem before any attempt runs
/// (e.g. `artifact_id` is empty). A runner-level failure, a cancellation, or a
/// regeneration error is surfaced as an `Ok(HealResult)` with
/// `outcome == HealOutcome::Error` and an `error_message`, so the UI always
/// receives the attempts made so far.
#[allow(clippy::too_many_lines)] // The bounded loop + its stop conditions read clearest inline.
pub async fn heal(
    request: HealRequest,
    gen_deps: &GenerationDeps<'_>,
    sandbox_deps: &SandboxDeps<'_>,
    mut on_event: Option<HealSink>,
) -> AppResult<HealResult> {
    let max = request.max_attempts.clamp(HEAL_MIN_ATTEMPTS, HEAL_MAX_ATTEMPTS);

    let mut current_artifact_id = request.artifact_id.trim().to_string();
    if current_artifact_id.is_empty() {
        return Err(crate::error::AppError::InvalidInput("artifactId is empty".into()));
    }

    let span = tracing::info_span!("self_heal", artifact_id = %current_artifact_id, max_attempts = max);
    let _enter = span.enter();

    let mut attempts: Vec<HealAttempt> = Vec::new();
    let mut best = Best::default();
    // The failing-test set of the previous attempt (sorted names), to detect
    // a model that is stuck producing the same failures.
    let mut prev_failing: Option<Vec<String>> = None;

    for attempt in 1..=max {
        // 1. Execute the current artifact through the existing sandbox entry
        //    point. A pre-flight `Err` (missing artifact, opt-out) aborts.
        let run = match sandbox_service::run(
            RunRequest {
                artifact_id: current_artifact_id.clone(),
                opt_in_confirmed: request.opt_in_confirmed,
                client_run_id: request.client_run_id.clone(),
            },
            sandbox_deps,
        )
        .await
        {
            Ok(run) => run,
            Err(err) => {
                // Only the very first run can hit this before any attempt is
                // recorded; later artifacts are produced by `generate` and are
                // structurally valid. Either way, surface it as a result.
                if attempts.is_empty() {
                    return Err(err);
                }
                return Ok(result_error(attempts, &best, &current_artifact_id, err.to_string()));
            }
        };

        // 2. Record the attempt + update the running "best".
        let failures = collect_failures(&run);
        attempts.push(HealAttempt {
            attempt,
            artifact_id: current_artifact_id.clone(),
            passed_count: run.passed_count,
            failed_count: run.failed_count,
            failures: failures.clone(),
        });
        best.consider(&run, &current_artifact_id);
        emit(&mut on_event, attempt, run.passed_count, run.failed_count);

        // 3. A runner-level failure or cancellation ends the heal (design §4.4).
        if run.status == RunStatus::Cancelled {
            let message = format!("Self-heal cancelled during attempt {attempt} of {max}.");
            return Ok(result_error(attempts, &best, &current_artifact_id, message));
        }
        if run.status == RunStatus::Error {
            let message = run
                .error_message
                .clone()
                .unwrap_or_else(|| format!("The sandbox run failed on attempt {attempt} of {max}."));
            return Ok(result_error(attempts, &best, &current_artifact_id, message));
        }

        // 4. All tests pass → healed (design §4.1).
        if run.failed_count == 0 {
            tracing::info!(attempt, passed = run.passed_count, "self-heal succeeded");
            return Ok(HealResult {
                outcome: HealOutcome::Healed,
                attempts_used: attempts_len(&attempts),
                final_artifact_id: current_artifact_id,
                final_run_id: run.run_id,
                passed_count: run.passed_count,
                failed_count: run.failed_count,
                attempts,
                error_message: None,
            });
        }

        // 5. Out of attempts → exhausted, landing on the best attempt
        //    (design §4.2).
        if attempt == max {
            tracing::info!(attempt, "self-heal exhausted attempt budget");
            return Ok(result_from_best(HealOutcome::Exhausted, attempts, &best));
        }

        // 6. The model is stuck if the failing-test set is identical to the
        //    previous attempt → bail early before another LLM call (design §4.3).
        let failing_names = sorted_names(&failures);
        if prev_failing.as_ref() == Some(&failing_names) {
            tracing::info!(attempt, "self-heal made no progress; bailing");
            return Ok(result_from_best(HealOutcome::NoProgress, attempts, &best));
        }
        prev_failing = Some(failing_names);

        // 7. Regenerate the whole artifact with synthesized feedback, chaining
        //    the version via `parent_id`. The new artifact becomes current.
        let feedback = synth_feedback(&failures);
        let regen = generation_service::generate(
            GenerationRequest {
                project_id: request.project_id.clone(),
                project_name: request.project_name.clone(),
                artifact_type: ArtifactType::TestCases,
                model: request.model.clone(),
                scope_hint: request.scope_hint.clone(),
                project_summary: request.project_summary.clone(),
                reviewer_feedback: feedback,
                parent_id: Some(current_artifact_id.clone()),
            },
            gen_deps,
            None,
        )
        .await;

        match regen {
            Ok(outcome) => current_artifact_id = outcome.artifact_id,
            Err(err) => {
                let message = format!("Regenerating the test cases failed on attempt {attempt}: {err}");
                return Ok(result_error(attempts, &best, &current_artifact_id, message));
            }
        }
    }

    // Unreachable: the `attempt == max` branch returns inside the loop and
    // `max >= HEAL_MIN_ATTEMPTS >= 1`. Kept so the function is total.
    Ok(result_from_best(HealOutcome::Exhausted, attempts, &best))
}

/// The failing tests of a run, mapped to the feedback-carrying shape.
fn collect_failures(run: &RunResult) -> Vec<HealFailure> {
    run.tests
        .iter()
        .filter(|t| t.status == TestStatus::Failed)
        .map(|t| HealFailure {
            name: t.name.clone(),
            failure_message: t.failure_message.clone(),
        })
        .collect()
}

/// Sorted, de-duplicated failing-test names — the comparable identity of an
/// attempt's failure set for the no-progress check.
fn sorted_names(failures: &[HealFailure]) -> Vec<String> {
    let mut names: Vec<String> = failures.iter().map(|f| f.name.clone()).collect();
    names.sort();
    names.dedup();
    names
}

fn attempts_len(attempts: &[HealAttempt]) -> u32 {
    u32::try_from(attempts.len()).unwrap_or(u32::MAX)
}

fn emit(sink: &mut Option<HealSink>, attempt: u32, passed: u32, failed: u32) {
    if let Some(sink) = sink.as_mut() {
        sink(HealEvent::Attempt { attempt, passed, failed });
    }
}

/// The best attempt seen so far — "highest `passed_count`; ties → latest"
/// (design §4). Because attempts are considered in order, `>=` keeps the later
/// of two equal-passing attempts.
#[derive(Default)]
struct Best {
    set: bool,
    passed_count: u32,
    failed_count: u32,
    artifact_id: String,
    run_id: String,
}

impl Best {
    fn consider(&mut self, run: &RunResult, artifact_id: &str) {
        if !self.set || run.passed_count >= self.passed_count {
            self.set = true;
            self.passed_count = run.passed_count;
            self.failed_count = run.failed_count;
            self.artifact_id = artifact_id.to_string();
            self.run_id.clone_from(&run.run_id);
        }
    }
}

/// Build a non-error result that lands the user on the best attempt.
fn result_from_best(outcome: HealOutcome, attempts: Vec<HealAttempt>, best: &Best) -> HealResult {
    HealResult {
        outcome,
        attempts_used: attempts_len(&attempts),
        final_artifact_id: best.artifact_id.clone(),
        final_run_id: best.run_id.clone(),
        passed_count: best.passed_count,
        failed_count: best.failed_count,
        attempts,
        error_message: None,
    }
}

/// Build an [`HealOutcome::Error`] result. Lands on the best attempt when one
/// exists, else on the artifact that failed pre-flight.
fn result_error(
    attempts: Vec<HealAttempt>,
    best: &Best,
    current_artifact_id: &str,
    message: String,
) -> HealResult {
    if best.set {
        let mut result = result_from_best(HealOutcome::Error, attempts, best);
        result.error_message = Some(message);
        return result;
    }
    HealResult {
        outcome: HealOutcome::Error,
        attempts_used: attempts_len(&attempts),
        final_artifact_id: current_artifact_id.to_string(),
        final_run_id: String::new(),
        passed_count: 0,
        failed_count: 0,
        attempts,
        error_message: Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::providers::embeddings::EmbeddingProvider;
    use crate::providers::llm::error::LlmError;
    use crate::providers::llm::types::{
        Chunk, FinishReason, GenerateRequest, ProviderCapabilities, Usage,
    };
    use crate::providers::llm::{approximate_token_count, ChunkStream, LlmProvider};
    // `RunStatus`, `TestStatus`, `RunRequest` are already in scope via `super::*`.
    use crate::providers::runners::{
        CancelToken, RunInput, RunnerError, RunnerLanguage, RunnerOutput, TestResult, TestRunner,
    };
    use crate::repositories::artifact_repo::{self, ArtifactInsert, ArtifactType, GenerationMetadata};
    use crate::repositories::chunk_repo::{self, ChunkInsert};
    use crate::services::chunking_service::{Chunk as CodeChunk, ChunkKind};
    use crate::services::sandbox_service::RunRegistry;
    use async_trait::async_trait;
    use chrono::Utc;
    use sqlx::SqlitePool;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, PoisonError};
    use uuid::Uuid;

    // ---- synth_feedback (pure) -------------------------------------------

    fn failure(name: &str, message: Option<&str>) -> HealFailure {
        HealFailure { name: name.into(), failure_message: message.map(str::to_string) }
    }

    #[test]
    fn synth_feedback_empty_is_just_the_instruction() {
        let out = synth_feedback(&[]);
        assert!(out.starts_with("The following generated test cases failed"));
        assert!(!out.contains("\n- "), "no bullet lines when there are no failures");
    }

    #[test]
    fn synth_feedback_single_failure_includes_name_and_message() {
        let out = synth_feedback(&[failure("TC-CART-09 (computes tax)", Some("expected 19.99 to equal 20.00"))]);
        assert!(out.contains("\n- TC-CART-09 (computes tax): expected 19.99 to equal 20.00"));
    }

    #[test]
    fn synth_feedback_missing_message_is_labelled() {
        let out = synth_feedback(&[failure("TC-X", None), failure("TC-Y", Some("   "))]);
        assert!(out.contains("\n- TC-X: (no failure message captured)"));
        // Whitespace-only messages are treated as missing too.
        assert!(out.contains("\n- TC-Y: (no failure message captured)"));
    }

    #[test]
    fn synth_feedback_truncates_long_messages() {
        let long = "x".repeat(MAX_FAILURE_MESSAGE_CHARS + 50);
        let out = synth_feedback(&[failure("TC-LONG", Some(&long))]);
        assert!(out.contains('…'), "an over-long message is ellipsized");
        assert!(!out.contains(&long), "the full untruncated message is not folded in");
    }

    #[test]
    fn synth_feedback_caps_failure_count_and_summarizes_overflow() {
        let many: Vec<HealFailure> = (0..MAX_FEEDBACK_FAILURES + 5)
            .map(|i| failure(&format!("TC-{i}"), Some("boom")))
            .collect();
        let out = synth_feedback(&many);
        let bullets = out.matches("\n- ").count();
        // MAX folded failures + one overflow summary line.
        assert_eq!(bullets, MAX_FEEDBACK_FAILURES + 1);
        assert!(out.contains("…and 5 more failing tests."));
    }

    // ---- scripted test doubles -------------------------------------------

    /// LLM whose `stream()` yields a different scripted chunk list each call,
    /// so a multi-pass heal loop (one `generate` per regeneration) is
    /// deterministic. Mirrors `SequencedLlm` in `generation_service` tests.
    #[derive(Clone)]
    struct SequencedLlm {
        capabilities: ProviderCapabilities,
        scripts: Arc<Mutex<VecDeque<Vec<Chunk>>>>,
    }

    impl SequencedLlm {
        fn new(scripts: Vec<Vec<Chunk>>) -> Self {
            Self {
                capabilities: ProviderCapabilities {
                    supports_tools: true,
                    supports_streaming: true,
                    max_context_tokens: 32_000,
                    max_output_tokens: 4_000,
                },
                scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for SequencedLlm {
        fn name(&self) -> &'static str {
            "scripted"
        }
        fn capabilities(&self) -> &ProviderCapabilities {
            &self.capabilities
        }
        fn count_tokens(&self, text: &str) -> usize {
            approximate_token_count(text)
        }
        fn stream(&self, _request: GenerateRequest) -> ChunkStream {
            let script = self
                .scripts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
                .unwrap_or_default();
            Box::pin(async_stream::stream! {
                for chunk in script {
                    yield Ok::<_, LlmError>(chunk);
                }
            })
        }
    }

    /// LLM whose `stream()` yields a single provider error — used to drive the
    /// "regeneration failed" branch deterministically.
    #[derive(Clone)]
    struct ErroringLlm {
        capabilities: ProviderCapabilities,
    }

    impl ErroringLlm {
        fn new() -> Self {
            Self {
                capabilities: ProviderCapabilities {
                    supports_tools: true,
                    supports_streaming: true,
                    max_context_tokens: 32_000,
                    max_output_tokens: 4_000,
                },
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ErroringLlm {
        fn name(&self) -> &'static str {
            "erroring"
        }
        fn capabilities(&self) -> &ProviderCapabilities {
            &self.capabilities
        }
        fn count_tokens(&self, text: &str) -> usize {
            approximate_token_count(text)
        }
        fn stream(&self, _request: GenerateRequest) -> ChunkStream {
            Box::pin(async_stream::stream! {
                yield Err(LlmError::InvalidResponse {
                    provider: "erroring",
                    message: "scripted failure".into(),
                });
            })
        }
    }

    #[derive(Clone)]
    struct ScriptedEmbeddings {
        dim: usize,
    }

    #[async_trait]
    impl EmbeddingProvider for ScriptedEmbeddings {
        fn name(&self) -> &'static str {
            "scripted-emb"
        }
        fn dimension(&self) -> usize {
            self.dim
        }
        #[allow(clippy::unnecessary_literal_bound)]
        fn model_id(&self) -> &str {
            "test-model"
        }
        async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, LlmError> {
            Ok(inputs.into_iter().map(|_| vec![1.0; self.dim]).collect())
        }
    }

    /// Runner that yields one queued outcome per `run()` — the heal loop runs
    /// the suite once per attempt. Mirrors `MultiScriptedRunner` in the flaky
    /// tests.
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

    fn out(status: RunStatus, tests: Vec<TestResult>) -> RunnerOutput {
        RunnerOutput { status, tests, coverage: vec![], stdout: String::new(), stderr: String::new() }
    }

    fn tc(name: &str, status: TestStatus, failure: Option<&str>) -> TestResult {
        TestResult {
            name: name.into(),
            status,
            duration_ms: 1,
            failure_message: failure.map(str::to_string),
            source_line: None,
        }
    }

    // ---- DB / artifact seeding -------------------------------------------

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-heal-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
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
        (pool, path)
    }

    /// One indexed chunk matching `ScriptedEmbeddings { dim: 8 }` so `generate`'s
    /// RAG retrieval returns a non-empty context (regeneration needs source).
    async fn seed_chunk(pool: &SqlitePool) {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO project_files (id, project_id, path, language, size_bytes, file_type, sha256, created_at, updated_at) \
             VALUES ('f1', 'p1', 'src/add.ts', 'typescript', 0, 'source', 'h', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("seed file");
        chunk_repo::insert_batch(
            pool,
            vec![ChunkInsert {
                project_id: "p1".into(),
                file_id: "f1".into(),
                chunk: CodeChunk {
                    kind: ChunkKind::Function,
                    name: "add".into(),
                    start_line: 1,
                    end_line: 3,
                    content: "export function add(a, b) { return a + b; }\n".into(),
                    token_count: 10,
                    oversize: false,
                },
                embedding: vec![1.0; 8],
                embedding_dim: 8,
                embedding_provider: "scripted-emb-test-model".into(),
                embedding_model: "test-model".into(),
            }],
        )
        .await
        .expect("seed chunk");
    }

    /// Seed a runnable test-cases artifact (carries a `files[]` workspace).
    async fn seed_artifact(pool: &SqlitePool) -> String {
        let now = Utc::now().to_rfc3339();
        artifact_repo::insert(
            pool,
            ArtifactInsert {
                project_id: "p1".into(),
                artifact_type: ArtifactType::TestCases,
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

    fn done_chunk(input: u32, output: u32) -> Chunk {
        Chunk::Done {
            usage: Usage { input_tokens: input, output_tokens: output },
            finish_reason: FinishReason::Stop,
        }
    }

    fn args_chunk(s: &str) -> Chunk {
        Chunk::ToolCallArgsDelta { id: "call_1".into(), json_fragment: s.into() }
    }

    /// A complete test-cases payload (cases + runnable `files[]`) so `generate`
    /// validates and skips the files-repair follow-up pass.
    fn test_cases_with_files_json() -> &'static str {
        r#"{
            "cases": [
                {
                    "id": "TC-ADD-POSITIVE",
                    "title": "add returns the sum of two numbers",
                    "type": "positive",
                    "priority": "p1",
                    "steps": [
                        {"action": "call add(1, 2)", "expectedResult": "returns 3"}
                    ]
                }
            ],
            "files": [
                {"path": "src/add.ts", "contents": "export function add(a, b) { return a + b; }", "isTest": false},
                {"path": "add.test.ts", "contents": "import { describe, it, expect } from 'vitest';\nimport { add } from './src/add';\ndescribe('add', () => { it('TC-ADD-POSITIVE', () => { expect(add(1, 2)).toBe(3); }); });", "isTest": true}
            ]
        }"#
    }

    fn regen_script() -> Vec<Chunk> {
        vec![args_chunk(test_cases_with_files_json()), done_chunk(100, 50)]
    }

    fn heal_request(artifact_id: String, max_attempts: u32) -> HealRequest {
        HealRequest {
            artifact_id,
            max_attempts,
            opt_in_confirmed: true,
            client_run_id: String::new(),
            model: "qwen2.5-coder:7b".into(),
            project_id: "p1".into(),
            project_name: "demo".into(),
            scope_hint: String::new(),
            project_summary: "Adder library.".into(),
        }
    }

    // ---- heal orchestrator -----------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_succeeds_on_second_attempt() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        // Attempt 1 fails; after one regeneration, attempt 2 passes.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-ADD-POSITIVE", TestStatus::Failed, Some("expected 3 to equal 4"))])),
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("TC-ADD-POSITIVE", TestStatus::Passed, None)])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![regen_script()]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = heal(heal_request(artifact_id.clone(), 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("heal runs");

        assert_eq!(result.outcome, HealOutcome::Healed);
        assert_eq!(result.attempts_used, 2);
        assert_eq!(result.failed_count, 0);
        assert_eq!(result.passed_count, 1);
        // Landed on the regenerated version, not the original.
        assert_ne!(result.final_artifact_id, artifact_id);
        assert!(!result.final_run_id.is_empty());
        // The healed test flipped from failing (attempt 1) to passing (attempt 2).
        assert_eq!(result.attempts[0].failed_count, 1);
        assert_eq!(result.attempts[1].failed_count, 0);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_exhausts_and_returns_best_attempt() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        // Attempt 1: 2 pass / 1 fail. Attempt 2 regresses: 1 pass / 2 fail.
        // Budget is 2, so the loop exhausts and must land on attempt 1 (the
        // higher pass count), not the latest.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(out(RunStatus::Failed, vec![
                tc("B", TestStatus::Passed, None),
                tc("C", TestStatus::Passed, None),
                tc("A", TestStatus::Failed, Some("nope")),
            ])),
            Scripted::Succeed(out(RunStatus::Failed, vec![
                tc("B", TestStatus::Passed, None),
                tc("C", TestStatus::Failed, Some("regressed")),
                tc("A", TestStatus::Failed, Some("still nope")),
            ])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![regen_script()]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = heal(heal_request(artifact_id.clone(), 2), &gen_deps, &sandbox_deps, None)
            .await
            .expect("heal runs");

        assert_eq!(result.outcome, HealOutcome::Exhausted);
        assert_eq!(result.attempts_used, 2);
        // Best attempt = attempt 1 (2 passing), the original artifact.
        assert_eq!(result.passed_count, 2);
        assert_eq!(result.failed_count, 1);
        assert_eq!(result.final_artifact_id, artifact_id);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_bails_on_no_progress() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        // Both attempts fail with the *same* failing-test set → the model is
        // stuck, so the loop bails before exhausting its 4-attempt budget.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("A", TestStatus::Failed, Some("v1"))])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("A", TestStatus::Failed, Some("v2"))])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![regen_script()]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = heal(heal_request(artifact_id, 4), &gen_deps, &sandbox_deps, None)
            .await
            .expect("heal runs");

        assert_eq!(result.outcome, HealOutcome::NoProgress);
        assert_eq!(result.attempts_used, 2);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_surfaces_a_run_error() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        // The first run errors (Docker down) → heal stops with an Error result,
        // never regenerating.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Fail(RunnerError::DockerUnavailable("daemon down".into())),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        // Empty script: if the loop wrongly regenerated, the LLM would panic on
        // an unscripted pop — but it returns default (empty) here, so assert the
        // error result instead.
        let llm = Arc::new(SequencedLlm::new(vec![]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = heal(heal_request(artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("heal returns a result, not Err");

        assert_eq!(result.outcome, HealOutcome::Error);
        assert_eq!(result.attempts_used, 1);
        assert!(result.error_message.is_some());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_surfaces_a_cancelled_run() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        let runner: Arc<dyn TestRunner> =
            Arc::new(MultiScriptedRunner::new(vec![Scripted::Fail(RunnerError::Cancelled)]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = heal(heal_request(artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("heal returns a result");

        assert_eq!(result.outcome, HealOutcome::Error);
        assert!(result.error_message.as_deref().unwrap_or_default().contains("cancelled"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_surfaces_a_regeneration_error() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        // Attempt 1 fails; the regeneration then errors (provider stream error)
        // → heal stops with an Error result after one attempt.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("A", TestStatus::Failed, Some("boom"))])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(ErroringLlm::new());
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = heal(heal_request(artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("heal returns a result");

        assert_eq!(result.outcome, HealOutcome::Error);
        assert_eq!(result.attempts_used, 1);
        assert!(result.error_message.unwrap().contains("Regenerating"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_streams_attempt_events() {
        let (pool, path) = seed_pool().await;
        seed_chunk(&pool).await;
        let artifact_id = seed_artifact(&pool).await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("A", TestStatus::Failed, Some("boom"))])),
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("A", TestStatus::Passed, None)])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![regen_script()]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let events = Arc::new(Mutex::new(Vec::<(u32, u32, u32)>::new()));
        let sink_events = events.clone();
        let sink: HealSink = Box::new(move |event| match event {
            HealEvent::Attempt { attempt, passed, failed } => {
                sink_events.lock().unwrap_or_else(PoisonError::into_inner).push((attempt, passed, failed));
            }
        });

        let result = heal(heal_request(artifact_id, 3), &gen_deps, &sandbox_deps, Some(sink))
            .await
            .expect("heal runs");

        assert_eq!(result.outcome, HealOutcome::Healed);
        let captured = events.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(captured, vec![(1, 0, 1), (2, 1, 0)]);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn heal_rejects_empty_artifact_id() {
        let (pool, path) = seed_pool().await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps = SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let err = heal(heal_request("   ".into(), 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect_err("blank artifact id is rejected pre-flight");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heal_outcome_str_round_trips() {
        for outcome in [HealOutcome::Healed, HealOutcome::Exhausted, HealOutcome::NoProgress, HealOutcome::Error] {
            assert_eq!(HealOutcome::from_str_value(outcome.as_str()), Some(outcome));
        }
        assert_eq!(HealOutcome::from_str_value("bogus"), None);
    }
}
