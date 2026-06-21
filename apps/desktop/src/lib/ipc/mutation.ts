import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  type ImproveRequest,
  ImproveRequestSchema,
  type ImproveResult,
  ImproveResultSchema,
  type ImproveStreamEvent,
  ImproveStreamEventSchema,
  type MutationCheckRecord,
  MutationCheckRecordSchema,
  type MutationCheckSummary,
  MutationCheckSummarySchema,
  type MutationResult,
  MutationResultSchema,
  type MutationStreamEvent,
  MutationStreamEventSchema,
  type RunRequest,
  RunRequestSchema,
} from '@testing-ide/shared';
import { z } from 'zod';

import { IpcError } from './error';
import { invokeAndParse } from './invoke';

/** Default mutant cap sent when the caller does not specify (matches the
 * backend default; re-clamped to [1, 200] there). */
const DEFAULT_MAX_MUTANTS = 40;

/** Default page size for mutation-score history (mirrors the backend default). */
const MUTATION_HISTORY_LIMIT = 20;

/**
 * Channel the backend emits per-mutant sweep progress on. Mirrored from
 * `commands/sandbox.rs::MUTATION_EVENT`.
 */
const MUTATION_EVENT_CHANNEL = 'mutation://event';

/**
 * Channel the backend emits per-attempt "improve coverage" progress on.
 * Mirrored from `commands/sandbox.rs::IMPROVE_EVENT`.
 */
const IMPROVE_EVENT_CHANNEL = 'improve://event';

/**
 * Mutation-test a generated test-case artifact: score how many seeded bugs the
 * suite catches (plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md, Stage 1).
 * Validates `args` against `RunRequestSchema` before sending. `maxMutants` is a
 * hint — the backend re-clamps it to [1, 200].
 *
 * Unlike a single run, a non-green baseline, a cancellation, or a runner death
 * mid-sweep **is** thrown as an `IpcError` (the score has no partial form). A
 * per-mutant build failure is not an error — it is just an excluded `errored`
 * mutant in the returned `MutationResult`.
 */
export async function runMutationTest(
  args: RunRequest,
  maxMutants: number = DEFAULT_MAX_MUTANTS,
): Promise<MutationResult> {
  const parsed = RunRequestSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('run_mutation_test', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('run_mutation_test', MutationResultSchema, {
    request: parsed.data,
    maxMutants,
  });
}

/**
 * List an artifact's persisted mutation-score history, newest first
 * (plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md §5.5). Returns header
 * summaries; fetch a check's per-mutant detail with [`getMutationCheck`].
 * `limit` is a hint — the backend re-clamps it to [1, 200].
 */
export async function listMutationChecks(
  artifactId: string,
  limit: number = MUTATION_HISTORY_LIMIT,
): Promise<MutationCheckSummary[]> {
  return invokeAndParse('list_mutation_checks', z.array(MutationCheckSummarySchema), {
    artifactId,
    limit,
  });
}

/**
 * Fetch one persisted mutation check with its per-mutant verdicts. Throws an
 * `IpcError` when the id is unknown (`NOT_FOUND`).
 */
export async function getMutationCheck(checkId: string): Promise<MutationCheckRecord> {
  return invokeAndParse('get_mutation_check', MutationCheckRecordSchema, { checkId });
}

/**
 * Subscribe to per-mutant sweep progress events. Returns an `unlisten` callback
 * the caller MUST invoke on unmount. Schema-invalid payloads are dropped
 * silently so a future backend event kind cannot crash the renderer.
 */
export async function subscribeToMutationEvents(
  handler: (event: MutationStreamEvent) => void,
): Promise<UnlistenFn> {
  try {
    return await listen<unknown>(MUTATION_EVENT_CHANNEL, (event) => {
      const parsed = MutationStreamEventSchema.safeParse(event.payload);
      if (parsed.success) {
        handler(parsed.data);
      }
    });
  } catch (err) {
    throw new IpcError(MUTATION_EVENT_CHANNEL, asMessage(err), { cause: err });
  }
}

/**
 * Auto-generate tests that kill a suite's surviving mutants and re-score to
 * prove the lift (plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md, Stage 2).
 * Validates `args` against `ImproveRequestSchema` before sending. `maxAttempts`
 * is a hint re-clamped to [1, 5]; `maxMutants` to [1, 200].
 *
 * A later score failure (the regenerated suite was not green) or a regeneration
 * error is **not** thrown — it comes back as an `ImproveResult` with `outcome:
 * 'error'` carrying an `errorMessage`. Only a pre-flight rejection (opt-out,
 * blank/missing artifact, unresolvable provider, or a **red baseline** on the
 * first score) throws an `IpcError`.
 */
export async function improveCoverage(args: ImproveRequest): Promise<ImproveResult> {
  const parsed = ImproveRequestSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('improve_coverage', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('improve_coverage', ImproveResultSchema, { request: parsed.data });
}

/**
 * Subscribe to per-attempt improve progress events. Returns an `unlisten`
 * callback the caller MUST invoke on unmount. Schema-invalid payloads are
 * dropped silently so a future backend event kind cannot crash the renderer.
 */
export async function subscribeToImproveEvents(
  handler: (event: ImproveStreamEvent) => void,
): Promise<UnlistenFn> {
  try {
    return await listen<unknown>(IMPROVE_EVENT_CHANNEL, (event) => {
      const parsed = ImproveStreamEventSchema.safeParse(event.payload);
      if (parsed.success) {
        handler(parsed.data);
      }
    });
  } catch (err) {
    throw new IpcError(IMPROVE_EVENT_CHANNEL, asMessage(err), { cause: err });
  }
}

function asMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  return JSON.stringify(err);
}
