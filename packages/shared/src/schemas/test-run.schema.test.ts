import { describe, expect, it } from 'vitest';

import {
  FlakyRunResultSchema,
  FlakyTestResultSchema,
  TestVerdictSchema,
} from './test-run.schema';

const UUID_A = '123e4567-e89b-12d3-a456-426614174000';

describe('TestVerdictSchema', () => {
  it('accepts the three serde verdict literals', () => {
    for (const verdict of ['stable_pass', 'stable_fail', 'flaky'] as const) {
      expect(TestVerdictSchema.parse(verdict)).toBe(verdict);
    }
  });

  it('rejects an unknown verdict', () => {
    expect(TestVerdictSchema.safeParse('unstable').success).toBe(false);
  });
});

describe('FlakyTestResultSchema', () => {
  it('round-trips a flaky row carrying a sample failure', () => {
    const parsed = FlakyTestResultSchema.parse({
      name: 'TC-CART-09 computes tax',
      verdict: 'flaky',
      passCount: 4,
      executedCount: 5,
      totalRuns: 5,
      sampleFailure: 'expected 19.99 to equal 20.00',
    });
    expect(parsed.verdict).toBe('flaky');
    expect(parsed.passCount).toBe(4);
    expect(parsed.sampleFailure).toBe('expected 19.99 to equal 20.00');
  });

  it('accepts a stable-pass row with sampleFailure omitted (serde None)', () => {
    const parsed = FlakyTestResultSchema.parse({
      name: 'TC-LOGIN-01 accepts valid credentials',
      verdict: 'stable_pass',
      passCount: 5,
      executedCount: 5,
      totalRuns: 5,
    });
    expect(parsed.sampleFailure).toBeUndefined();
  });

  it('rejects totalRuns below 1 — a check always runs at least twice', () => {
    expect(
      FlakyTestResultSchema.safeParse({
        name: 't',
        verdict: 'flaky',
        passCount: 0,
        executedCount: 0,
        totalRuns: 0,
      }).success,
    ).toBe(false);
  });
});

describe('FlakyRunResultSchema', () => {
  it('round-trips a completed check with a per-test verdict list', () => {
    const parsed = FlakyRunResultSchema.parse({
      runId: UUID_A,
      totalRuns: 5,
      flakyCount: 1,
      nonFlakyCount: 1,
      tests: [
        {
          name: 'TC-LOGIN-01 accepts valid credentials',
          verdict: 'stable_pass',
          passCount: 5,
          executedCount: 5,
          totalRuns: 5,
        },
        {
          name: 'TC-CART-09 computes tax',
          verdict: 'flaky',
          passCount: 4,
          executedCount: 5,
          totalRuns: 5,
          sampleFailure: 'expected 19.99 to equal 20.00',
        },
      ],
    });
    expect(parsed.flakyCount).toBe(1);
    expect(parsed.tests).toHaveLength(2);
    expect(parsed.errorMessage).toBeUndefined();
  });

  it('accepts a verdict-less error result with an empty runId', () => {
    // The backend returns an empty runId + errorMessage when an iteration
    // errors before iteration #1 is persisted (design §4); runId is a plain
    // string, not a uuid, so the Zod mirror must not require uuid here.
    const parsed = FlakyRunResultSchema.parse({
      runId: '',
      totalRuns: 5,
      flakyCount: 0,
      nonFlakyCount: 0,
      tests: [],
      errorMessage: 'Flaky check failed on run 2 of 5: [DOCKER_UNAVAILABLE] docker unavailable',
    });
    expect(parsed.runId).toBe('');
    expect(parsed.tests).toHaveLength(0);
    expect(parsed.errorMessage).toContain('DOCKER_UNAVAILABLE');
  });
});
