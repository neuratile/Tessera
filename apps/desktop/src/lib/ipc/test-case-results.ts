import {
  type TestCaseResult,
  TestCaseResultSchema,
  type UpsertTestCaseResultInput,
  UpsertTestCaseResultInputSchema,
} from '@testing-ide/shared';
import { z } from 'zod';

import { IpcError } from './error';
import { invokeAndParse, invokeVoid } from './invoke';

/**
 * List every stored execution outcome (Actual output / Result + remarks)
 * for a test-cases artifact, so the Test Case table can LEFT JOIN them
 * onto the LLM cases on mount (plan/TEST_CASE_TABLE.md §4.1).
 */
export async function listTestCaseResults(artifactId: string): Promise<TestCaseResult[]> {
  return invokeAndParse('list_test_case_results', z.array(TestCaseResultSchema), { artifactId });
}

/**
 * Upsert one manual outcome. Validates `input` against
 * `UpsertTestCaseResultInputSchema` before sending so callers fail fast
 * on bad input. The backend records this as a `manual`-source row; a
 * later sandbox run on the same case overwrites it (last writer wins).
 */
export async function upsertTestCaseResult(input: UpsertTestCaseResultInput): Promise<void> {
  const parsed = UpsertTestCaseResultInputSchema.safeParse(input);
  if (!parsed.success) {
    throw new IpcError('upsert_test_case_result', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeVoid('upsert_test_case_result', { input: parsed.data });
}
