import { describe, expect, it } from 'vitest';

import {
  HealAttemptSchema,
  HealFailureSchema,
  HealOutcomeSchema,
  HealRequestSchema,
  HealResultSchema,
} from './heal.schema';

describe('HealOutcomeSchema', () => {
  it('accepts the four serde outcome literals', () => {
    for (const outcome of ['healed', 'exhausted', 'no_progress', 'error'] as const) {
      expect(HealOutcomeSchema.parse(outcome)).toBe(outcome);
    }
  });

  it('rejects an unknown outcome', () => {
    expect(HealOutcomeSchema.safeParse('partially_healed').success).toBe(false);
  });
});

describe('HealFailureSchema', () => {
  it('round-trips a failure carrying a message', () => {
    const parsed = HealFailureSchema.parse({
      name: 'TC-CART-09 computes tax',
      failureMessage: 'expected 19.99 to equal 20.00',
    });
    expect(parsed.failureMessage).toBe('expected 19.99 to equal 20.00');
  });

  it('accepts a failure with failureMessage omitted (serde None)', () => {
    const parsed = HealFailureSchema.parse({ name: 'TC-X' });
    expect(parsed.failureMessage).toBeUndefined();
  });
});

describe('HealAttemptSchema', () => {
  it('round-trips an attempt with its failure trail', () => {
    const parsed = HealAttemptSchema.parse({
      attempt: 1,
      artifactId: 'a-1',
      passedCount: 13,
      failedCount: 1,
      failures: [{ name: 'TC-CART-07', failureMessage: 'expected 45.00 to equal 50.00' }],
    });
    expect(parsed.attempt).toBe(1);
    expect(parsed.failures).toHaveLength(1);
  });

  it('rejects attempt 0 — attempts are 1-based', () => {
    expect(
      HealAttemptSchema.safeParse({
        attempt: 0,
        artifactId: 'a-1',
        passedCount: 0,
        failedCount: 0,
        failures: [],
      }).success,
    ).toBe(false);
  });
});

describe('HealResultSchema', () => {
  it('round-trips a healed result with no error message', () => {
    const parsed = HealResultSchema.parse({
      outcome: 'healed',
      attemptsUsed: 2,
      finalArtifactId: 'a-2',
      finalRunId: 'r-2',
      passedCount: 14,
      failedCount: 0,
      attempts: [
        { attempt: 1, artifactId: 'a-1', passedCount: 13, failedCount: 1, failures: [{ name: 'TC-CART-09' }] },
        { attempt: 2, artifactId: 'a-2', passedCount: 14, failedCount: 0, failures: [] },
      ],
      // errorMessage omitted — serde None
    });
    expect(parsed.outcome).toBe('healed');
    expect(parsed.attempts).toHaveLength(2);
    expect(parsed.errorMessage).toBeUndefined();
  });

  it('round-trips an error result carrying an error message', () => {
    const parsed = HealResultSchema.parse({
      outcome: 'error',
      attemptsUsed: 1,
      finalArtifactId: 'a-1',
      finalRunId: '',
      passedCount: 0,
      failedCount: 0,
      attempts: [{ attempt: 1, artifactId: 'a-1', passedCount: 0, failedCount: 0, failures: [] }],
      errorMessage: 'The sandbox run failed on attempt 1 of 3.',
    });
    expect(parsed.outcome).toBe('error');
    expect(parsed.errorMessage).toContain('failed');
  });
});

describe('HealRequestSchema', () => {
  it('round-trips a request with the optional serde-default fields omitted', () => {
    const parsed = HealRequestSchema.parse({
      artifactId: 'a-1',
      maxAttempts: 3,
      optInConfirmed: true,
      model: 'qwen2.5-coder:7b',
      provider: 'ollama',
      projectId: 'p-1',
      projectName: 'demo',
    });
    expect(parsed.clientRunId).toBeUndefined();
    expect(parsed.scopeHint).toBeUndefined();
    expect(parsed.projectSummary).toBeUndefined();
  });

  it('rejects a request missing the opt-in flag', () => {
    expect(
      HealRequestSchema.safeParse({
        artifactId: 'a-1',
        maxAttempts: 3,
        model: 'm',
        provider: 'ollama',
        projectId: 'p-1',
        projectName: 'demo',
      }).success,
    ).toBe(false);
  });
});
