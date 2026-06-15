import { z } from 'zod';

/**
 * Contract for the closed-loop sandboxed test runner
 * (plan/versions/v1/SANDBOX_TEST_RUNNER.md §6).
 *
 * Rust serde is the source of truth (rules §12.3.1); these schemas mirror
 * the structs + enums in
 * `apps/desktop/src-tauri/src/providers/runners/mod.rs`. Status literals
 * must match the Rust `snake_case` serde output (single-word lowercase)
 * and the TEXT stored in the `test_runs` / `test_run_cases` tables exactly.
 */

/**
 * Lifecycle status of a run — mirrors the Rust `RunStatus` enum and the
 * `test_runs.status` column.
 */
export const RunStatusSchema = z.union([
  z.literal('pending'),
  z.literal('running'),
  z.literal('passed'),
  z.literal('failed'),
  z.literal('error'),
  z.literal('cancelled'),
]);

export type RunStatus = z.infer<typeof RunStatusSchema>;

/**
 * Outcome of a single executed assertion — mirrors the Rust `TestStatus`
 * enum and the `test_run_cases.status` column.
 */
export const TestStatusSchema = z.union([
  z.literal('passed'),
  z.literal('failed'),
  z.literal('skipped'),
]);

export type TestStatus = z.infer<typeof TestStatusSchema>;

/**
 * IPC request to execute a generated test-case artifact in the sandbox —
 * mirrors the Rust `RunRequest`. `optInConfirmed` must be `true`; the
 * backend rejects runs when execution is opted out (plan §3).
 */
export const RunRequestSchema = z.object({
  artifactId: z.string().uuid(),
  optInConfirmed: z.boolean(),
  /**
   * Caller-generated correlation id the backend registers the run's cancel
   * token under, so the UI can Stop a run before the run IPC returns.
   * Optional on the wire (the Rust struct defaults it to empty); the UI
   * always supplies a UUID via `crypto.randomUUID()`. Validated as any
   * non-empty string — the Rust `RunRequest` is a plain `String`, and the
   * Zod mirror must not be stricter than the serde source of truth
   * (rules.md §12.3.1).
   */
  clientRunId: z.string().min(1).optional(),
});

export type RunRequest = z.infer<typeof RunRequestSchema>;

/**
 * One executed test assertion — mirrors the Rust `TestResult`.
 * `failureMessage` / `sourceLine` are present only for failures; the
 * backend omits them otherwise. `sourceLine` is 1-based.
 */
export const TestResultSchema = z.object({
  name: z.string().min(1),
  status: TestStatusSchema,
  durationMs: z.number().int().nonnegative(),
  failureMessage: z.string().optional(),
  sourceLine: z.number().int().positive().optional(),
});

export type TestResult = z.infer<typeof TestResultSchema>;

/**
 * Coverage hit-count for one source line — mirrors the Rust
 * `CoverageLine`. `hits === 0` marks an uncovered line; `line` is 1-based.
 */
export const CoverageLineSchema = z.object({
  filePath: z.string().min(1),
  line: z.number().int().positive(),
  hits: z.number().int().nonnegative(),
});

export type CoverageLine = z.infer<typeof CoverageLineSchema>;

/**
 * Aggregate result of a run — mirrors the Rust `RunResult`. Returned to
 * the renderer and persisted across the `test_runs` family of tables.
 */
export const RunResultSchema = z.object({
  runId: z.string().uuid(),
  status: RunStatusSchema,
  passedCount: z.number().int().nonnegative(),
  failedCount: z.number().int().nonnegative(),
  durationMs: z.number().int().nonnegative(),
  tests: z.array(TestResultSchema),
  coverage: z.array(CoverageLineSchema),
  errorMessage: z.string().optional(),
});

export type RunResult = z.infer<typeof RunResultSchema>;

/**
 * Verdict for one test across the N runs of a flaky check — mirrors the Rust
 * `TestVerdict` enum (plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md
 * §5.1). `stable_pass` = passed (or skipped) every run; `stable_fail` = a
 * real reproducible failure; `flaky` = mixed pass/fail.
 */
export const TestVerdictSchema = z.union([
  z.literal('stable_pass'),
  z.literal('stable_fail'),
  z.literal('flaky'),
]);

export type TestVerdict = z.infer<typeof TestVerdictSchema>;

/**
 * Per-test outcome of a flaky check — mirrors the Rust `FlakyTestResult`.
 * `passCount / executedCount` is the "passed X/N" ratio; `executedCount`
 * excludes runs where the test was skipped. `sampleFailure` is present only
 * when the test failed at least once (the backend omits it otherwise).
 */
export const FlakyTestResultSchema = z.object({
  name: z.string().min(1),
  verdict: TestVerdictSchema,
  passCount: z.number().int().nonnegative(),
  executedCount: z.number().int().nonnegative(),
  totalRuns: z.number().int().positive(),
  sampleFailure: z.string().optional(),
});

export type FlakyTestResult = z.infer<typeof FlakyTestResultSchema>;

/**
 * Aggregate result of a flaky check — mirrors the Rust `FlakyRunResult`.
 * `runId` is iteration #1, persisted via the normal run path so the check
 * appears in run history. `errorMessage` is present (and `tests` empty) when
 * an iteration errored or the check was cancelled before completing.
 */
export const FlakyRunResultSchema = z.object({
  runId: z.string(),
  totalRuns: z.number().int().positive(),
  flakyCount: z.number().int().nonnegative(),
  /**
   * Every test that was *not* flaky — both `stable_pass` and `stable_fail`.
   * Named `nonFlakyCount` (not `stableCount`) so it cannot be misread as
   * "reliably passing": a deterministically failing test is non-flaky but is
   * certainly not passing.
   */
  nonFlakyCount: z.number().int().nonnegative(),
  tests: z.array(FlakyTestResultSchema),
  errorMessage: z.string().optional(),
});

export type FlakyRunResult = z.infer<typeof FlakyRunResultSchema>;

/**
 * One entry in an artifact's persisted flaky-check history — mirrors the Rust
 * `FlakyCheckSummary` (plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md
 * §7). A lightweight header for the trend list; the per-test verdicts are
 * fetched on demand as a `FlakyCheckRecord`. `runId` is the iteration-#1 run
 * the check persisted, omitted (serde `None`) only if that run row was later
 * purged. `createdAt` is RFC-3339. `flakyCount + nonFlakyCount` is the test
 * total — every test is exactly one of the two.
 */
export const FlakyCheckSummarySchema = z.object({
  id: z.string().uuid(),
  runId: z.string().uuid().optional(),
  totalRuns: z.number().int().positive(),
  flakyCount: z.number().int().nonnegative(),
  nonFlakyCount: z.number().int().nonnegative(),
  createdAt: z.string().min(1),
});

export type FlakyCheckSummary = z.infer<typeof FlakyCheckSummarySchema>;

/**
 * A persisted flaky check with its full per-test verdict list — mirrors the
 * Rust `FlakyCheckRecord`. The detail behind a `FlakyCheckSummary`, rendered
 * with the same per-test UI as a live check.
 */
export const FlakyCheckRecordSchema = FlakyCheckSummarySchema.extend({
  tests: z.array(FlakyTestResultSchema),
});

export type FlakyCheckRecord = z.infer<typeof FlakyCheckRecordSchema>;
