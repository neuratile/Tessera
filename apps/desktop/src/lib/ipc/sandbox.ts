import {
  type FlakyRunResult,
  FlakyRunResultSchema,
  type RunRequest,
  RunRequestSchema,
  type RunResult,
  RunResultSchema,
} from '@testing-ide/shared';
import { z } from 'zod';

import { IpcError } from './error';
import { invokeAndParse } from './invoke';

/**
 * Execute a generated test-case artifact in the local Docker sandbox and
 * return the persisted [`RunResult`]. Validates `args` against
 * `RunRequestSchema` before sending so callers fail fast on bad input.
 *
 * A runner-level failure (Docker down, timeout) is **not** an exception —
 * it comes back as a `RunResult` with `status: 'error'` carrying an
 * `errorMessage`. Only pre-flight rejections (opt-out, missing/wrong-type
 * artifact, no runnable files) throw an `IpcError`.
 */
export async function runTestSandbox(args: RunRequest): Promise<RunResult> {
  const parsed = RunRequestSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('run_test_sandbox', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('run_test_sandbox', RunResultSchema, { request: parsed.data });
}

/**
 * Run a generated test-case artifact `runs` times in the local Docker sandbox
 * and classify each test as stable-pass / stable-fail / flaky
 * (plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md). `runs` is a hint
 * — the backend re-clamps it to [2, 20].
 *
 * A runner-level failure or a cancellation mid-check is **not** an exception —
 * it comes back as a `FlakyRunResult` with an `errorMessage` and no verdicts.
 * Only pre-flight rejections (opt-out, missing/wrong-type artifact, no runnable
 * files) throw an `IpcError`.
 */
export async function runTestSandboxFlaky(
  args: RunRequest,
  runs: number,
): Promise<FlakyRunResult> {
  const parsed = RunRequestSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('run_test_sandbox_flaky', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeAndParse('run_test_sandbox_flaky', FlakyRunResultSchema, {
    request: parsed.data,
    runs,
  });
}

/**
 * Request cancellation of an in-flight run by its `clientRunId` (the UI Stop
 * button). Resolves to `true` when a live run matched, `false` when it had
 * already finished — both benign.
 */
export async function cancelTestSandbox(clientRunId: string): Promise<boolean> {
  return invokeAndParse('cancel_test_sandbox', z.boolean(), { runId: clientRunId });
}
