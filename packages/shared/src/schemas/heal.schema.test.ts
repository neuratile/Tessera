import { describe, expect, it } from 'vitest';

import {
  HealAttemptSchema,
  HealCheckRecordSchema,
  HealCheckSummarySchema,
  HealFailureSchema,
  HealOutcomeSchema,
  HealRequestSchema,
  HealResultSchema,
  HealTestRecordSchema,
  HealTestStatusSchema,
} from './heal.schema';

const SUMMARY = {
  id: '11111111-1111-4111-8111-111111111111',
  landedRunId: '22222222-2222-4222-8222-222222222222',
  landedVersionId: 'a-2',
  attempts: 2,
  healedCount: 1,
  stillFailingCount: 0,
  finalPassing: 14,
  finalTotal: 14,
  createdAt: '2026-06-24T10:00:00Z',
} as const;

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

describe('HealTestStatusSchema', () => {
  it('accepts the three serde status literals', () => {
    for (const status of ['healed', 'still_failing', 'passed'] as const) {
      expect(HealTestStatusSchema.parse(status)).toBe(status);
    }
  });

  it('rejects an unknown status', () => {
    expect(HealTestStatusSchema.safeParse('regressed').success).toBe(false);
  });
});

describe('HealTestRecordSchema', () => {
  it('round-trips a healed test carrying its heal attempt + last failure', () => {
    const parsed = HealTestRecordSchema.parse({
      name: 'TC-CART-09',
      status: 'healed',
      healedAtAttempt: 2,
      lastFailureMessage: 'expected 19.99 to equal 20.00',
    });
    expect(parsed.status).toBe('healed');
    expect(parsed.healedAtAttempt).toBe(2);
  });

  it('accepts a still-failing test with the optional fields omitted', () => {
    const parsed = HealTestRecordSchema.parse({ name: 'TC-X', status: 'still_failing' });
    expect(parsed.healedAtAttempt).toBeUndefined();
    expect(parsed.lastFailureMessage).toBeUndefined();
  });
});

describe('HealCheckSummarySchema', () => {
  it('round-trips a history header', () => {
    const parsed = HealCheckSummarySchema.parse(SUMMARY);
    expect(parsed.attempts).toBe(2);
    expect(parsed.healedCount).toBe(1);
    expect(parsed.landedRunId).toBe(SUMMARY.landedRunId);
  });

  it('accepts a summary with landedRunId omitted (the run was purged)', () => {
    const parsed = HealCheckSummarySchema.parse({
      id: SUMMARY.id,
      landedVersionId: SUMMARY.landedVersionId,
      attempts: SUMMARY.attempts,
      healedCount: SUMMARY.healedCount,
      stillFailingCount: SUMMARY.stillFailingCount,
      finalPassing: SUMMARY.finalPassing,
      finalTotal: SUMMARY.finalTotal,
      createdAt: SUMMARY.createdAt,
    });
    expect(parsed.landedRunId).toBeUndefined();
  });
});

describe('HealCheckRecordSchema', () => {
  it('round-trips a record with its per-test detail', () => {
    const parsed = HealCheckRecordSchema.parse({
      ...SUMMARY,
      tests: [
        { name: 'TC-A', status: 'healed', healedAtAttempt: 2, lastFailureMessage: 'boom' },
        { name: 'TC-B', status: 'still_failing', lastFailureMessage: 'still red' },
      ],
    });
    expect(parsed.tests).toHaveLength(2);
    expect(parsed.tests[0]?.status).toBe('healed');
  });
});
