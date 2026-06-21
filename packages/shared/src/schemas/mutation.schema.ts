import { z } from 'zod';

/**
 * Contract for Stage 1 mutation testing
 * (plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md §5.6).
 *
 * Rust serde is the source of truth (rules §12.3.1); these schemas mirror the
 * structs + enums in
 * `apps/desktop/src-tauri/src/providers/runners/mutation.rs`. Status literals
 * must match the Rust `snake_case` serde output exactly, and the TEXT stored in
 * the `mutation_checks` / `mutation_check_mutants` tables (migration 0009).
 */

/**
 * Outcome of running the suite against one mutant — mirrors the Rust
 * `MutantStatus` enum.
 *
 * - `killed`   — the suite failed against the mutant; the bug was caught.
 * - `survived` — the suite still passed; a real gap.
 * - `errored`  — the mutant did not build/run; excluded from the score
 *   denominator (a mutant that won't compile proves nothing about the suite).
 */
export const MutantStatusSchema = z.union([
  z.literal('killed'),
  z.literal('survived'),
  z.literal('errored'),
]);

export type MutantStatus = z.infer<typeof MutantStatusSchema>;

/**
 * A single-edit mutation of one source file — mirrors the Rust `Mutant` struct.
 * `original → replacement` is shown in the survivor list (e.g. `>` → `>=`).
 * `byteStart` / `byteEnd` are only meaningful against the exact baseline source
 * and are not persisted in history (the repo reads them back as 0).
 */
export const MutantSchema = z.object({
  file: z.string().min(1),
  line: z.number().int().positive(),
  operatorId: z.string().min(1),
  original: z.string(),
  replacement: z.string(),
  byteStart: z.number().int().nonnegative(),
  byteEnd: z.number().int().nonnegative(),
});

export type Mutant = z.infer<typeof MutantSchema>;

/**
 * One mutant paired with the suite's verdict against it — mirrors the Rust
 * `MutantResult` struct.
 */
export const MutantResultSchema = z.object({
  mutant: MutantSchema,
  status: MutantStatusSchema,
});

export type MutantResult = z.infer<typeof MutantResultSchema>;

/**
 * Aggregate result of a mutation-score run — mirrors the Rust `MutationResult`.
 * `score = killed / (killed + survived)` in `[0, 1]` (errored mutants leave the
 * denominator); `0` when nothing was scorable (`total === 0`). `total =
 * killed + survived + errored`. `droppedCount` is how many generated mutants the
 * cap sampled out (never silently truncated).
 */
export const MutationResultSchema = z.object({
  score: z.number().min(0).max(1),
  killed: z.number().int().nonnegative(),
  survived: z.number().int().nonnegative(),
  errored: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  baselineRunId: z.string(),
  mutants: z.array(MutantResultSchema),
  droppedCount: z.number().int().nonnegative(),
});

export type MutationResult = z.infer<typeof MutationResultSchema>;

/**
 * One entry in an artifact's persisted mutation-score history — mirrors the Rust
 * `MutationCheckSummary`. A lightweight header for the "Mutation history" trend;
 * the per-mutant detail is fetched on demand as a `MutationCheckRecord`.
 * `baselineRunId` is omitted (serde `None`) only if that run row was later
 * purged. `createdAt` is RFC-3339.
 */
export const MutationCheckSummarySchema = z.object({
  id: z.string().uuid(),
  baselineRunId: z.string().uuid().optional(),
  score: z.number().min(0).max(1),
  killed: z.number().int().nonnegative(),
  survived: z.number().int().nonnegative(),
  errored: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  droppedCount: z.number().int().nonnegative(),
  createdAt: z.string().min(1),
});

export type MutationCheckSummary = z.infer<typeof MutationCheckSummarySchema>;

/**
 * A persisted mutation check with its full per-mutant list — mirrors the Rust
 * `MutationCheckRecord`. The detail behind a `MutationCheckSummary`, re-rendered
 * with the same survivor UI as a live check.
 */
export const MutationCheckRecordSchema = MutationCheckSummarySchema.extend({
  mutants: z.array(MutantResultSchema),
});

export type MutationCheckRecord = z.infer<typeof MutationCheckRecordSchema>;

/**
 * Per-mutant progress event streamed on the `mutation://event` channel —
 * mirrors the Rust `MutationEventPayload` struct in `commands/sandbox.rs`.
 * `kind` is always `'mutant'` in Stage 1; it is kept so the renderer can pivot
 * on future event kinds without a schema change. `mutationId` correlates events
 * to one sweep when several are in flight.
 */
export const MutationStreamEventSchema = z.object({
  mutationId: z.string(),
  kind: z.literal('mutant'),
  done: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
});

export type MutationStreamEvent = z.infer<typeof MutationStreamEventSchema>;

/* -------------------------------------------------------------------------- */
/* Stage 2 — "Improve coverage" (MUTATION_TESTING.md §5.1, §5.6).             */
/*                                                                            */
/* Mirrors the Rust structs + enums in                                        */
/* `apps/desktop/src-tauri/src/services/mutation_service.rs` (results) and     */
/* `commands/sandbox.rs` (the request + event wire forms).                     */
/* -------------------------------------------------------------------------- */

/**
 * Terminal state of an improve loop — mirrors the Rust `ImproveOutcome` enum.
 *
 * - `improved`    — the mutation score rose but did not reach 100%.
 * - `perfect`     — every scorable mutant is now killed (100%).
 * - `exhausted`   — the attempt budget ran out with no net gain.
 * - `no_progress` — a regeneration failed to raise the score; the loop bailed.
 * - `error`       — a score sweep failed / was cancelled, or a regen errored.
 */
export const ImproveOutcomeSchema = z.union([
  z.literal('improved'),
  z.literal('perfect'),
  z.literal('exhausted'),
  z.literal('no_progress'),
  z.literal('error'),
]);

export type ImproveOutcome = z.infer<typeof ImproveOutcomeSchema>;

/**
 * Record of one score → (maybe) regenerate cycle — mirrors the Rust
 * `ImproveAttempt` struct. `artifactId` is the version that was *scored* on this
 * attempt; `score` is its mutation score in `[0, 1]`.
 */
export const ImproveAttemptSchema = z.object({
  attempt: z.number().int().positive(),
  artifactId: z.string().min(1),
  score: z.number().min(0).max(1),
  killed: z.number().int().nonnegative(),
  survived: z.number().int().nonnegative(),
});

export type ImproveAttempt = z.infer<typeof ImproveAttemptSchema>;

/**
 * Aggregate result of an improve loop — mirrors the Rust `ImproveResult` struct.
 * `finalArtifactId` is the best-scoring version the user lands on; `startScore`
 * / `finalScore` carry the lift. `errorMessage` is set only when
 * `outcome === 'error'`.
 */
export const ImproveResultSchema = z.object({
  outcome: ImproveOutcomeSchema,
  attemptsUsed: z.number().int().nonnegative(),
  finalArtifactId: z.string(),
  startScore: z.number().min(0).max(1),
  finalScore: z.number().min(0).max(1),
  attempts: z.array(ImproveAttemptSchema),
  errorMessage: z.string().optional(),
});

export type ImproveResult = z.infer<typeof ImproveResultSchema>;

/**
 * IPC request to run the improve loop — mirrors the Rust `ImproveArgs` struct in
 * `commands/sandbox.rs` (camelCase). `clientRunId`, `scopeHint`, and
 * `projectSummary` carry Rust `#[serde(default)]`, so they are optional here.
 * `maxAttempts` is a hint re-clamped to `[1, 5]`; `maxMutants` to `[1, 200]`.
 */
export const ImproveRequestSchema = z.object({
  artifactId: z.string().min(1),
  maxAttempts: z.number().int().positive(),
  maxMutants: z.number().int().positive(),
  optInConfirmed: z.boolean(),
  clientRunId: z.string().optional(),
  model: z.string().min(1),
  provider: z.string().min(1),
  projectId: z.string().min(1),
  projectName: z.string().min(1),
  scopeHint: z.string().optional(),
  projectSummary: z.string().optional(),
});

export type ImproveRequest = z.infer<typeof ImproveRequestSchema>;

/**
 * Per-attempt progress event streamed on the `improve://event` channel —
 * mirrors the Rust `ImproveEventPayload` struct in `commands/sandbox.rs`. `kind`
 * is always `'attempt'`; it is kept so the renderer can pivot on future event
 * kinds without a schema change. `improveId` correlates events to one loop when
 * several are in flight.
 */
export const ImproveStreamEventSchema = z.object({
  improveId: z.string(),
  kind: z.literal('attempt'),
  attempt: z.number().int().positive(),
  score: z.number().min(0).max(1),
});

export type ImproveStreamEvent = z.infer<typeof ImproveStreamEventSchema>;
