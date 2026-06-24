import { z } from 'zod';

/**
 * Contract for the agentic self-healing loop
 * (plan/versions/v2/v2-feature-docs/SELF_HEALING_LOOP.md §5).
 *
 * Rust serde is the source of truth (rules §12.3.1); these schemas mirror the
 * structs + enums in `apps/desktop/src-tauri/src/services/healing_service.rs`
 * (results) and `commands/healing.rs` (the request wire form). Outcome
 * literals must match the Rust `snake_case` serde output exactly.
 */

/**
 * Terminal state of a heal loop — mirrors the Rust `HealOutcome` enum.
 *
 * - `healed`      — every test passed on the final attempt.
 * - `exhausted`   — the attempt budget ran out; the best attempt is returned.
 * - `no_progress` — the failing-test set stopped changing; the loop bailed.
 * - `error`       — a run failed / was cancelled, or a regeneration errored.
 */
export const HealOutcomeSchema = z.union([
  z.literal('healed'),
  z.literal('exhausted'),
  z.literal('no_progress'),
  z.literal('error'),
]);

export type HealOutcome = z.infer<typeof HealOutcomeSchema>;

/**
 * One failing test carried forward into the next attempt's feedback — mirrors
 * the Rust `HealFailure` struct. `failureMessage` is omitted from the wire
 * payload when absent (Rust `skip_serializing_if`).
 */
export const HealFailureSchema = z.object({
  name: z.string().min(1),
  failureMessage: z.string().optional(),
});

export type HealFailure = z.infer<typeof HealFailureSchema>;

/**
 * Record of one run → (maybe) regenerate cycle — mirrors the Rust
 * `HealAttempt` struct. `artifactId` is the version that was *run* on this
 * attempt.
 */
export const HealAttemptSchema = z.object({
  attempt: z.number().int().positive(),
  artifactId: z.string().min(1),
  passedCount: z.number().int().nonnegative(),
  failedCount: z.number().int().nonnegative(),
  failures: z.array(HealFailureSchema),
});

export type HealAttempt = z.infer<typeof HealAttemptSchema>;

/**
 * Aggregate result of a heal loop — mirrors the Rust `HealResult` struct.
 * `finalArtifactId` / `finalRunId` point at the version the user lands on (the
 * healed attempt, or the best attempt by pass count). `errorMessage` is set
 * only when `outcome === 'error'`.
 */
export const HealResultSchema = z.object({
  outcome: HealOutcomeSchema,
  attemptsUsed: z.number().int().nonnegative(),
  finalArtifactId: z.string(),
  finalRunId: z.string(),
  passedCount: z.number().int().nonnegative(),
  failedCount: z.number().int().nonnegative(),
  attempts: z.array(HealAttemptSchema),
  errorMessage: z.string().optional(),
});

export type HealResult = z.infer<typeof HealResultSchema>;

/**
 * IPC request to run the self-healing loop — mirrors the Rust `HealArgs`
 * struct in `commands/healing.rs` (camelCase). `clientRunId`, `scopeHint`, and
 * `projectSummary` carry Rust `#[serde(default)]`, so they are optional here.
 * `maxAttempts` is a hint: the backend re-clamps it to `[1, 5]`.
 */
export const HealRequestSchema = z.object({
  artifactId: z.string().min(1),
  maxAttempts: z.number().int().positive(),
  optInConfirmed: z.boolean(),
  clientRunId: z.string().optional(),
  model: z.string().min(1),
  provider: z.string().min(1),
  projectId: z.string().min(1),
  projectName: z.string().min(1),
  scopeHint: z.string().optional(),
  projectSummary: z.string().optional(),
});

export type HealRequest = z.infer<typeof HealRequestSchema>;

/**
 * Per-attempt progress event streamed on the `heal://event` channel — mirrors
 * the Rust `HealEventPayload` struct in `commands/healing.rs`. `kind` is always
 * `'attempt'` in this slice; it is kept so the renderer can pivot on future
 * event kinds without a schema change. `healId` correlates events to one heal
 * run when several are in flight.
 */
export const HealStreamEventSchema = z.object({
  healId: z.string(),
  kind: z.literal('attempt'),
  attempt: z.number().int().positive(),
  passed: z.number().int().nonnegative(),
  failed: z.number().int().nonnegative(),
});

export type HealStreamEvent = z.infer<typeof HealStreamEventSchema>;

/* -------------------------------------------------------------------------- */
/* Heal history (v2 hardening — V2_HARDENING.md §5.1).                        */
/*                                                                            */
/* Mirrors the Rust DTOs + enum in                                            */
/* `apps/desktop/src-tauri/src/repositories/heal_check_repo.rs`. Status       */
/* literals must match the Rust `snake_case` serde output exactly, and the    */
/* TEXT stored in the `heal_checks` / `heal_check_tests` tables (migration     */
/* 0010).                                                                      */
/* -------------------------------------------------------------------------- */

/**
 * Verdict of one test within a persisted heal — mirrors the Rust
 * `HealTestStatus` enum.
 *
 * - `healed`        — failed in an earlier attempt but passes in the landed one.
 * - `still_failing` — still failing in the landed attempt (a likely real bug).
 * - `passed`        — passed throughout. Reserved for forward-compat; no writer
 *   emits it yet (a `HealResult` carries only the failing tests per attempt).
 */
export const HealTestStatusSchema = z.union([
  z.literal('healed'),
  z.literal('still_failing'),
  z.literal('passed'),
]);

export type HealTestStatus = z.infer<typeof HealTestStatusSchema>;

/**
 * One test involved in a heal, paired with its verdict — mirrors the Rust
 * `HealTestRecord`. `healedAtAttempt` is present only for a `healed` test (the
 * attempt it first passed); both it and `lastFailureMessage` are omitted from
 * the wire payload when absent (Rust `skip_serializing_if`).
 */
export const HealTestRecordSchema = z.object({
  name: z.string().min(1),
  status: HealTestStatusSchema,
  healedAtAttempt: z.number().int().positive().optional(),
  lastFailureMessage: z.string().optional(),
});

export type HealTestRecord = z.infer<typeof HealTestRecordSchema>;

/**
 * One entry in an artifact's persisted heal history — mirrors the Rust
 * `HealCheckSummary`. A lightweight header for the "Heal history" trend; the
 * per-test detail is fetched on demand as a `HealCheckRecord`. `landedRunId` is
 * omitted (serde `None`) only if that run row was later purged. `createdAt` is
 * RFC-3339.
 */
export const HealCheckSummarySchema = z.object({
  id: z.string().uuid(),
  landedRunId: z.string().uuid().optional(),
  landedVersionId: z.string().min(1),
  attempts: z.number().int().nonnegative(),
  healedCount: z.number().int().nonnegative(),
  stillFailingCount: z.number().int().nonnegative(),
  finalPassing: z.number().int().nonnegative(),
  finalTotal: z.number().int().nonnegative(),
  createdAt: z.string().min(1),
});

export type HealCheckSummary = z.infer<typeof HealCheckSummarySchema>;

/**
 * A persisted heal check with its full per-test list — mirrors the Rust
 * `HealCheckRecord`. The detail behind a `HealCheckSummary`, re-rendered with
 * the same per-test trail the live result view derives.
 */
export const HealCheckRecordSchema = HealCheckSummarySchema.extend({
  tests: z.array(HealTestRecordSchema),
});

export type HealCheckRecord = z.infer<typeof HealCheckRecordSchema>;
