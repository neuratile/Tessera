//! Mutation-testing orchestrator — Stage 1 (score) + Stage 2 (improve)
//! (`plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md`).
//!
//! Mutation testing measures what line coverage cannot: *would the suite fail
//! if the code were wrong?* [`score`] runs the suite once for a green baseline,
//! mutates each **covered** source line with a single small edit, reruns the
//! unchanged suite per mutant, and reports a kill/survive **mutation score**.
//!
//! [`improve`] (Stage 2) is the second half: it feeds the survivors — the
//! seeded bugs the suite missed — back to the LLM as `reviewer_feedback`,
//! regenerates the test-cases artifact (chaining the version via `parent_id`),
//! re-scores, and keeps the best version, bounded by an attempt budget. It
//! mirrors [`healing_service::heal`](crate::services::healing_service) move for
//! move, swapping "all tests pass" for "mutation score rose" — so it holds both
//! the [`GenerationDeps`] and [`SandboxDeps`] bundles, since neither sibling
//! service may call the other (layering, `rules.md` §4.2).
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

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::providers::runners::mutation::{
    apply_mutant, cap_mutants, generate_mutants, MutantResult, MutantStatus, MutationCheckRecord,
    MutationCheckSummary, MutationResult,
};
use crate::providers::runners::{RunInput, RunRequest, RunStatus, RunnerError, WorkspaceFile};
use crate::repositories::artifact_repo::ArtifactType;
use crate::repositories::mutation_check_repo::{self, MutationCheckInsert};
use crate::services::generation_service::{self, GenerationDeps, GenerationRequest};
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

// ---------------------------------------------------------------------------
// Stage 2 — "Improve coverage" (design §2 Stage 2, §5.1, §5.3).
// ---------------------------------------------------------------------------

/// Lower / upper bounds on the number of improve attempts (design §4). The UI
/// value is only a hint — the backend re-clamps so a tampered IPC payload
/// cannot force an unbounded regenerate loop (mirrors self-heal / flaky).
pub const IMPROVE_MIN_ATTEMPTS: u32 = 1;
pub const IMPROVE_MAX_ATTEMPTS: u32 = 5;
/// Default attempt budget when the caller supplies none (design §4).
pub const IMPROVE_DEFAULT_ATTEMPTS: u32 = 3;

/// Cap on how many survivors are folded into one feedback block so the
/// synthesized `reviewer_feedback` stays well within `generate`'s token budget
/// (mirrors `healing_service`'s failure cap).
const MAX_FEEDBACK_SURVIVORS: usize = 20;

/// Terminal state of an improve loop (design §5.1). `snake_case` wire form
/// mirrors the sibling status enums and the Zod literals in
/// `mutation.schema.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImproveOutcome {
    /// The mutation score rose above the starting score but did not reach 100%.
    Improved,
    /// Every scorable mutant is now killed — a perfect mutation score.
    Perfect,
    /// The attempt budget ran out with no net score gain; the best (== start)
    /// version is returned.
    Exhausted,
    /// A regeneration failed to raise the score, so the loop bailed early
    /// rather than burn more LLM calls, with no net gain.
    NoProgress,
    /// A score sweep failed / was cancelled, or a regeneration errored;
    /// `error_message` carries the detail.
    Error,
}

impl ImproveOutcome {
    /// Stable string used in IPC payloads. Matches the serde `snake_case` wire
    /// form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Improved => "improved",
            Self::Perfect => "perfect",
            Self::Exhausted => "exhausted",
            Self::NoProgress => "no_progress",
            Self::Error => "error",
        }
    }

    /// Inverse of [`as_str`](Self::as_str). Returns `None` for any unrecognised
    /// string.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "improved" => Some(Self::Improved),
            "perfect" => Some(Self::Perfect),
            "exhausted" => Some(Self::Exhausted),
            "no_progress" => Some(Self::NoProgress),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Record of one score → (maybe) regenerate cycle (design §5.1). `artifact_id`
/// is the version that was *scored* on this attempt. Mirrors
/// `ImproveAttemptSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImproveAttempt {
    pub attempt: u32,
    pub artifact_id: String,
    pub score: f64,
    pub killed: u32,
    pub survived: u32,
}

/// Aggregate result of an improve loop (design §5.1). `final_artifact_id` is the
/// best-scoring version the user lands on; `start_score` / `final_score` carry
/// the lift. `error_message` is set only when `outcome == Error`. Mirrors
/// `ImproveResultSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImproveResult {
    pub outcome: ImproveOutcome,
    pub attempts_used: u32,
    pub final_artifact_id: String,
    pub start_score: f64,
    pub final_score: f64,
    pub attempts: Vec<ImproveAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Inputs to [`improve`]. The regeneration params (`model`, `project_*`,
/// `scope_hint`, `project_summary`) are exactly what `generate` needs; the
/// provider/LLM itself is resolved by the command into [`GenerationDeps`]
/// before this is built. `max_mutants` is forwarded to each inner [`score`].
#[derive(Debug, Clone)]
pub struct ImproveRequest {
    pub artifact_id: String,
    pub max_attempts: u32,
    pub max_mutants: u32,
    pub opt_in_confirmed: bool,
    pub client_run_id: String,
    pub model: String,
    pub project_id: String,
    pub project_name: String,
    pub scope_hint: String,
    pub project_summary: String,
}

/// Progress event delivered to the optional [`ImproveSink`] after each attempt's
/// score, so the UI can stream "attempt 2 of 3 · re-scoring…".
#[derive(Debug, Clone, Copy)]
pub enum ImproveEvent {
    Attempt { attempt: u32, score: f64 },
}

/// Per-event hook the caller can supply to relay attempt progress to the UI.
/// Forwarding is best-effort — the loop continues even if the closure errors.
pub type ImproveSink = Box<dyn FnMut(ImproveEvent) + Send>;

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

// ---------------------------------------------------------------------------
// Stage 2 — the bounded "improve coverage" loop.
// ---------------------------------------------------------------------------

/// Run the bounded "improve coverage" loop over `request.artifact_id`
/// (design §2 Stage 2). Each attempt [`score`]s the current artifact, feeds its
/// survivors back to [`generation_service::generate`] as `reviewer_feedback`,
/// and re-scores the regenerated version, keeping the best mutation score.
///
/// Reuses both entry points verbatim: every inner sweep goes through [`score`]
/// (which itself runs the opt-in-gated, green-baseline-gated sandbox path and
/// persists each score as history), and every regeneration chains the version
/// via `parent_id` exactly as self-heal does.
///
/// # Errors
///
/// Returns `Err` only for a pre-flight problem before any attempt is recorded:
/// a blank `artifact_id`, or whatever the very first [`score`] rejects up front
/// — opt-out, a missing / wrong-type artifact, or a **red baseline** (mutation
/// scoring needs an all-green suite, so the user must fix or self-heal it
/// first). A later score failure (the regenerated suite was not green, or the
/// sweep was cancelled) or a regeneration error is surfaced as an
/// `Ok(ImproveResult)` with `outcome == ImproveOutcome::Error` and an
/// `error_message`, so the UI always receives the attempts made so far.
#[allow(clippy::too_many_lines)] // The bounded loop + its stop conditions read clearest inline.
pub async fn improve(
    request: ImproveRequest,
    gen_deps: &GenerationDeps<'_>,
    sandbox_deps: &SandboxDeps<'_>,
    mut on_event: Option<ImproveSink>,
) -> AppResult<ImproveResult> {
    let max = request.max_attempts.clamp(IMPROVE_MIN_ATTEMPTS, IMPROVE_MAX_ATTEMPTS);

    let mut current_artifact_id = request.artifact_id.trim().to_string();
    if current_artifact_id.is_empty() {
        return Err(AppError::InvalidInput("artifactId is empty".into()));
    }

    let span =
        tracing::info_span!("mutation_improve", artifact_id = %current_artifact_id, max_attempts = max);
    let _enter = span.enter();

    let mut attempts: Vec<ImproveAttempt> = Vec::new();
    let mut best = BestScore::default();
    let mut start_score = 0.0_f64;
    // The score of the previous attempt, to detect a regeneration that did not
    // help (the model is stuck) and bail before another LLM call.
    let mut prev_score: Option<f64> = None;

    for attempt in 1..=max {
        // 1. Score the current artifact through Stage 1: it runs the green-gated
        //    baseline, sweeps mutants, and persists the check. No inner
        //    per-mutant streaming — the improve UI streams per *attempt*.
        let scored = match score(
            RunRequest {
                artifact_id: current_artifact_id.clone(),
                opt_in_confirmed: request.opt_in_confirmed,
                client_run_id: request.client_run_id.clone(),
            },
            request.max_mutants,
            sandbox_deps,
            None,
        )
        .await
        {
            Ok(scored) => scored,
            Err(err) => {
                // The very first score can hit a pre-flight problem (opt-out,
                // missing artifact, or a RED baseline the user must heal first):
                // propagate as `Err` so the UI shows the actionable message. A
                // later score errored — the regenerated suite was not green, or
                // the sweep was cancelled — so keep the best version reached.
                if attempts.is_empty() {
                    return Err(err);
                }
                return Ok(improve_error(
                    attempts,
                    &best,
                    start_score,
                    &current_artifact_id,
                    err.to_string(),
                ));
            }
        };

        if attempt == 1 {
            start_score = scored.score;
        }

        let survivors: Vec<MutantResult> = scored
            .mutants
            .iter()
            .filter(|m| m.status == MutantStatus::Survived)
            .cloned()
            .collect();

        attempts.push(ImproveAttempt {
            attempt,
            artifact_id: current_artifact_id.clone(),
            score: scored.score,
            killed: scored.killed,
            survived: scored.survived,
        });
        emit_improve(&mut on_event, attempt, scored.score);
        best.consider(&current_artifact_id, &scored);

        // 2. Perfect — every scorable mutant was killed → nothing left to chase.
        if scored.survived == 0 && scored.total > 0 {
            tracing::info!(attempt, score = scored.score, "improve reached a perfect score");
            return Ok(improve_outcome(ImproveOutcome::Perfect, attempts, &best, start_score));
        }

        // No survivors to feed back (e.g. no mutable operators on covered lines):
        // there is nothing to regenerate toward, so stop on the best version.
        if survivors.is_empty() {
            return Ok(improve_outcome(
                lift_outcome(best.score, start_score, false),
                attempts,
                &best,
                start_score,
            ));
        }

        // 3. Out of attempts → stop, landing on the best-scoring version.
        if attempt == max {
            tracing::info!(attempt, "improve exhausted its attempt budget");
            return Ok(improve_outcome(
                lift_outcome(best.score, start_score, true),
                attempts,
                &best,
                start_score,
            ));
        }

        // 4. The previous regeneration did not raise the score → the model is
        //    stuck, so bail before burning another LLM call (mirrors heal's
        //    no-progress guard, keyed on score instead of the failing-test set).
        if let Some(previous) = prev_score {
            if scored.score <= previous {
                tracing::info!(attempt, "improve made no progress; bailing");
                return Ok(improve_outcome(
                    lift_outcome(best.score, start_score, false),
                    attempts,
                    &best,
                    start_score,
                ));
            }
        }
        prev_score = Some(scored.score);

        // 5. Synthesize the survivors into instructive feedback and regenerate
        //    the whole artifact, chaining the version via `parent_id`. The new
        //    artifact becomes current and is re-scored on the next iteration.
        let feedback = synth_survivor_feedback(&survivors);
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
                let message =
                    format!("Regenerating the test cases failed on attempt {attempt}: {err}");
                return Ok(improve_error(attempts, &best, start_score, &current_artifact_id, message));
            }
        }
    }

    // Unreachable: the `attempt == max` branch returns inside the loop and
    // `max >= IMPROVE_MIN_ATTEMPTS >= 1`. Kept so the function is total.
    Ok(improve_outcome(
        lift_outcome(best.score, start_score, true),
        attempts,
        &best,
        start_score,
    ))
}

/// Synthesize the survivors into one instructive `reviewer_feedback` block for
/// the next regeneration (design §2 Stage 2, step 5). **Pure** — the
/// unit-testable core of the improve feedback path.
///
/// The number of survivors folded in is capped at [`MAX_FEEDBACK_SURVIVORS`] so
/// the result stays within `generate`'s token budget; an overflow is summarized
/// as a trailing "…and N more uncaught mutations" line.
#[must_use]
pub fn synth_survivor_feedback(survivors: &[MutantResult]) -> String {
    let mut out = String::from(
        "Your test suite passed, but it FAILED to catch the following deliberately \
         introduced bugs (mutations) in the source under test. Add or strengthen test \
         cases so the suite would FAIL if each change were present. Do not delete or \
         weaken existing tests, and keep every assertion correct — these are real gaps, \
         not test bugs.",
    );

    for entry in survivors.iter().take(MAX_FEEDBACK_SURVIVORS) {
        let m = &entry.mutant;
        out.push_str("\n- ");
        out.push_str(&m.file);
        out.push(':');
        out.push_str(&m.line.to_string());
        out.push_str(" — `");
        out.push_str(&m.original);
        out.push_str("` was changed to `");
        out.push_str(&m.replacement);
        out.push_str("` and no test failed (");
        out.push_str(survivor_guidance(&m.operator_id));
        out.push_str(").");
    }

    if survivors.len() > MAX_FEEDBACK_SURVIVORS {
        let extra = survivors.len() - MAX_FEEDBACK_SURVIVORS;
        out.push_str("\n- …and ");
        out.push_str(&extra.to_string());
        out.push_str(" more uncaught mutations.");
    }

    out
}

/// A short, actionable hint for how to kill a survivor, keyed by operator kind.
fn survivor_guidance(operator_id: &str) -> &'static str {
    match operator_id {
        "arithmetic" => "assert the exact computed result",
        "relational" => "add a boundary / edge-case test",
        "logical" => "exercise both sides of the condition",
        "boolean_literal" => "assert the affected branch",
        "return_negation" => "assert the returned value",
        _ => "add a test that detects this change",
    }
}

/// Choose the non-perfect, non-error terminal outcome (design §5.1). A net
/// positive lift is [`ImproveOutcome::Improved`]; otherwise
/// [`ImproveOutcome::Exhausted`] when the whole attempt budget was spent, or
/// [`ImproveOutcome::NoProgress`] when the loop bailed early.
fn lift_outcome(best_score: f64, start_score: f64, budget_exhausted: bool) -> ImproveOutcome {
    if best_score > start_score {
        ImproveOutcome::Improved
    } else if budget_exhausted {
        ImproveOutcome::Exhausted
    } else {
        ImproveOutcome::NoProgress
    }
}

/// The best-scoring attempt seen so far — "highest mutation score; ties →
/// latest" (design §4). Because attempts are considered in order, `>=` keeps the
/// later of two equal-scoring attempts, guarding against a regeneration that
/// kills new survivors but resurrects old ones.
#[derive(Default)]
struct BestScore {
    set: bool,
    score: f64,
    artifact_id: String,
}

impl BestScore {
    fn consider(&mut self, artifact_id: &str, scored: &MutationResult) {
        if !self.set || scored.score >= self.score {
            self.set = true;
            self.score = scored.score;
            self.artifact_id = artifact_id.to_string();
        }
    }
}

/// Build a non-error result that lands the user on the best-scoring version.
fn improve_outcome(
    outcome: ImproveOutcome,
    attempts: Vec<ImproveAttempt>,
    best: &BestScore,
    start_score: f64,
) -> ImproveResult {
    ImproveResult {
        outcome,
        attempts_used: attempts_len(&attempts),
        final_artifact_id: best.artifact_id.clone(),
        start_score,
        final_score: best.score,
        attempts,
        error_message: None,
    }
}

/// Build an [`ImproveOutcome::Error`] result. Lands on the best version when one
/// exists, else on the artifact that failed.
fn improve_error(
    attempts: Vec<ImproveAttempt>,
    best: &BestScore,
    start_score: f64,
    current_artifact_id: &str,
    message: String,
) -> ImproveResult {
    if best.set {
        let mut result = improve_outcome(ImproveOutcome::Error, attempts, best, start_score);
        result.error_message = Some(message);
        return result;
    }
    ImproveResult {
        outcome: ImproveOutcome::Error,
        attempts_used: attempts_len(&attempts),
        final_artifact_id: current_artifact_id.to_string(),
        start_score,
        final_score: 0.0,
        attempts,
        error_message: Some(message),
    }
}

fn attempts_len(attempts: &[ImproveAttempt]) -> u32 {
    u32::try_from(attempts.len()).unwrap_or(u32::MAX)
}

fn emit_improve(sink: &mut Option<ImproveSink>, attempt: u32, score: f64) {
    if let Some(sink) = sink.as_mut() {
        sink(ImproveEvent::Attempt { attempt, score });
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
    use crate::providers::runners::mutation::Mutant;
    use crate::providers::runners::{
        CancelToken, CoverageLine, RunnerError, RunnerLanguage, RunnerOutput, TestResult,
        TestRunner, TestStatus,
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
        seed_chunk(&pool).await;

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
        seed_chunk(&pool).await;

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
        seed_chunk(&pool).await;

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
        seed_chunk(&pool).await;

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

    // ======================================================================
    // Stage 2 — improve loop.
    // ======================================================================

    // ---- synth_survivor_feedback (pure) ----------------------------------

    fn survivor(file: &str, line: u32, operator_id: &str, original: &str, replacement: &str) -> MutantResult {
        MutantResult {
            mutant: Mutant {
                file: file.into(),
                line,
                operator_id: operator_id.into(),
                original: original.into(),
                replacement: replacement.into(),
                byte_start: 0,
                byte_end: 0,
            },
            status: MutantStatus::Survived,
        }
    }

    #[test]
    fn synth_survivor_feedback_lists_each_survivor_with_guidance() {
        let out = synth_survivor_feedback(&[
            survivor("cart.ts", 42, "relational", ">", ">="),
            survivor("tax.ts", 18, "boolean_literal", "true", "false"),
        ]);
        assert!(out.contains("FAILED to catch"));
        assert!(out.contains(
            "\n- cart.ts:42 — `>` was changed to `>=` and no test failed (add a boundary / edge-case test)."
        ));
        assert!(out.contains(
            "\n- tax.ts:18 — `true` was changed to `false` and no test failed (assert the affected branch)."
        ));
    }

    #[test]
    fn synth_survivor_feedback_caps_and_summarizes_overflow() {
        let many: Vec<MutantResult> = (0..MAX_FEEDBACK_SURVIVORS + 4)
            .map(|i| survivor("a.ts", u32::try_from(i).unwrap_or(0) + 1, "arithmetic", "+", "-"))
            .collect();
        let out = synth_survivor_feedback(&many);
        let bullets = out.matches("\n- ").count();
        // Capped folds + one overflow summary line.
        assert_eq!(bullets, MAX_FEEDBACK_SURVIVORS + 1);
        assert!(out.contains("…and 4 more uncaught mutations."));
    }

    #[test]
    fn improve_outcome_str_round_trips() {
        for outcome in [
            ImproveOutcome::Improved,
            ImproveOutcome::Perfect,
            ImproveOutcome::Exhausted,
            ImproveOutcome::NoProgress,
            ImproveOutcome::Error,
        ] {
            assert_eq!(ImproveOutcome::from_str_value(outcome.as_str()), Some(outcome));
        }
        assert_eq!(ImproveOutcome::from_str_value("bogus"), None);
    }

    #[test]
    fn improve_result_serializes_camel_case() {
        let result = ImproveResult {
            outcome: ImproveOutcome::Improved,
            attempts_used: 2,
            final_artifact_id: "a2".into(),
            start_score: 0.5,
            final_score: 0.9,
            attempts: vec![ImproveAttempt {
                attempt: 1,
                artifact_id: "a1".into(),
                score: 0.5,
                killed: 1,
                survived: 1,
            }],
            error_message: None,
        };
        let value = serde_json::to_value(&result).expect("serialize");
        assert_eq!(value["outcome"], "improved");
        assert_eq!(value["finalArtifactId"], "a2");
        assert_eq!(value["startScore"], 0.5);
        assert_eq!(value["finalScore"], 0.9);
        assert_eq!(value["attempts"][0]["artifactId"], "a1");
        assert!(value.get("errorMessage").is_none(), "errorMessage omitted when None");
    }

    // ---- scripted LLM / embeddings (mirror healing_service tests) ---------

    /// LLM whose `stream()` yields a different scripted chunk list each call,
    /// so a multi-pass improve loop (one `generate` per regeneration) is
    /// deterministic.
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

    /// LLM whose `stream()` yields a single provider error — drives the
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

    /// One indexed chunk matching `ScriptedEmbeddings { dim: 8 }` so `generate`'s
    /// RAG retrieval (when used) returns a non-empty context.
    async fn seed_chunk(pool: &SqlitePool) {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO project_files (id, project_id, path, language, size_bytes, file_type, sha256, created_at, updated_at) \
             VALUES ('f1', 'p1', 'src/calc.ts', 'typescript', 0, 'source', 'h', ?, ?)",
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
                    name: "f".into(),
                    start_line: 1,
                    end_line: 1,
                    content: "export const f = (a, b) => a + b;\n".into(),
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

    fn done_chunk(input: u32, output: u32) -> Chunk {
        Chunk::Done {
            usage: Usage { input_tokens: input, output_tokens: output },
            finish_reason: FinishReason::Stop,
        }
    }

    fn args_chunk(s: &str) -> Chunk {
        Chunk::ToolCallArgsDelta { id: "call_1".into(), json_fragment: s.into() }
    }

    /// A regenerated test-cases payload whose source (`a > 0`) yields exactly
    /// one relational mutant, so each post-regeneration score is a single mutant
    /// run. Carries complete cases + runnable `files[]` so `generate` validates
    /// and skips the files-repair follow-up pass.
    fn regen_cases_json() -> &'static str {
        r#"{
            "cases": [
                {
                    "id": "TC-F-POSITIVE",
                    "title": "f returns true for positive input",
                    "type": "positive",
                    "priority": "p1",
                    "steps": [
                        {"action": "call f(1)", "expectedResult": "returns true"}
                    ]
                }
            ],
            "files": [
                {"path": "src/calc.ts", "contents": "export const f = (a) => a > 0;", "isTest": false},
                {"path": "calc.test.ts", "contents": "import { describe, it, expect } from 'vitest';\nimport { f } from './src/calc';\ndescribe('f', () => { it('TC-F-POSITIVE', () => { expect(f(1)).toBe(true); }); });", "isTest": true}
            ]
        }"#
    }

    fn regen_script() -> Vec<Chunk> {
        vec![args_chunk(regen_cases_json()), done_chunk(100, 50)]
    }

    fn improve_request(artifact_id: &str, max_attempts: u32) -> ImproveRequest {
        ImproveRequest {
            artifact_id: artifact_id.into(),
            max_attempts,
            max_mutants: 40,
            opt_in_confirmed: true,
            client_run_id: String::new(),
            model: "qwen2.5-coder:7b".into(),
            project_id: "p1".into(),
            project_name: "demo".into(),
            scope_hint: String::new(),
            project_summary: "Calculator library.".into(),
        }
    }

    // ---- improve orchestrator --------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn improve_reaches_perfect_after_regeneration() {
        let (pool, path) = open_pool().await;        // The initial suite leaves the arithmetic mutant alive (it survives).
        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;
        seed_chunk(&pool).await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            // Attempt 1 score: baseline passes; the lone mutant survives.
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("TC-1 ok", TestStatus::Passed)], vec![])),
            // Attempt 2 score (regenerated suite): baseline passes; mutant killed.
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps =
            SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![regen_script()]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = improve(improve_request(&artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("improve runs");

        assert_eq!(result.outcome, ImproveOutcome::Perfect);
        assert_eq!(result.attempts_used, 2);
        assert!((result.start_score - 0.0).abs() < f64::EPSILON);
        assert!((result.final_score - 1.0).abs() < f64::EPSILON);
        assert_ne!(result.final_artifact_id, artifact_id, "landed on the regenerated version");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn improve_streams_one_event_per_attempt() {
        let (pool, path) = open_pool().await;        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;
        seed_chunk(&pool).await;

        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("TC-1 ok", TestStatus::Passed)], vec![])),
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps =
            SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![regen_script()]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let attempts_seen = Arc::new(Mutex::new(Vec::<u32>::new()));
        let sink_attempts = attempts_seen.clone();
        let sink: ImproveSink = Box::new(move |event| match event {
            ImproveEvent::Attempt { attempt, .. } => {
                sink_attempts.lock().unwrap_or_else(PoisonError::into_inner).push(attempt);
            }
        });

        let result = improve(improve_request(&artifact_id, 3), &gen_deps, &sandbox_deps, Some(sink))
            .await
            .expect("improve runs");

        assert_eq!(result.outcome, ImproveOutcome::Perfect);
        let captured = attempts_seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(captured, vec![1, 2]);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn improve_is_perfect_immediately_without_regenerating() {
        let (pool, path) = open_pool().await;        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;
        seed_chunk(&pool).await;

        // Baseline passes; the one mutant is killed → already perfect, so no
        // regeneration happens (the LLM is never called).
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps =
            SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![])); // must never be called
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = improve(improve_request(&artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("improve runs");

        assert_eq!(result.outcome, ImproveOutcome::Perfect);
        assert_eq!(result.attempts_used, 1);
        assert_eq!(result.final_artifact_id, artifact_id);
        assert!((result.final_score - 1.0).abs() < f64::EPSILON);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn improve_aborts_on_a_red_baseline() {
        let (pool, path) = open_pool().await;        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;
        seed_chunk(&pool).await;

        // The first score's baseline is red → mutation scoring is refused, and
        // since no attempt was recorded, improve surfaces it as a pre-flight Err.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![Scripted::Succeed(
            out(RunStatus::Failed, vec![tc("TC-1 ok", TestStatus::Failed)], vec![]),
        )]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps =
            SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(SequencedLlm::new(vec![]));
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let err = improve(improve_request(&artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect_err("a red baseline aborts improve up front");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn improve_surfaces_a_regeneration_error_keeping_best() {
        let (pool, path) = open_pool().await;        let artifact_id = seed(&pool, "export const f = (a, b) => a + b;").await;
        seed_chunk(&pool).await;

        // Attempt 1 leaves a survivor; the regeneration then errors → improve
        // stops with an Error result that still lands on the best (only) version.
        let runner: Arc<dyn TestRunner> = Arc::new(MultiScriptedRunner::new(vec![
            Scripted::Succeed(pass_baseline("src/calc.ts", &[1])),
            Scripted::Succeed(out(RunStatus::Passed, vec![tc("TC-1 ok", TestStatus::Passed)], vec![])),
        ]));
        let registry = RunRegistry::new();
        let factory = fixed_factory(runner);
        let sandbox_deps =
            SandboxDeps { pool: &pool, crypto: None, runner_factory: &factory, registry: &registry };

        let llm = Arc::new(ErroringLlm::new());
        let embeddings: Arc<dyn EmbeddingProvider> = Arc::new(ScriptedEmbeddings { dim: 8 });
        let gen_deps = GenerationDeps { pool: &pool, llm, embeddings };

        let result = improve(improve_request(&artifact_id, 3), &gen_deps, &sandbox_deps, None)
            .await
            .expect("improve returns a result, not Err");

        assert_eq!(result.outcome, ImproveOutcome::Error);
        assert_eq!(result.attempts_used, 1);
        assert_eq!(result.final_artifact_id, artifact_id);
        assert!(result.error_message.unwrap_or_default().contains("Regenerating"));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
