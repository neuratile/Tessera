import { z } from 'zod';

/**
 * Contract for the closed-loop sandboxed test runner
 * (plan/SANDBOX_TEST_RUNNER.md §6).
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
   * Caller-generated correlation id (UUID) the backend registers the run's
   * cancel token under, so the UI can Stop a run before the run IPC returns.
   * Optional on the wire (the Rust struct defaults it to empty); the UI
   * always supplies one so its Stop button can target the run.
   */
  clientRunId: z.string().uuid().optional(),
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
