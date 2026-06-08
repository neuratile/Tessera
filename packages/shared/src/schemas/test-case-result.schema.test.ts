import { describe, expect, it } from 'vitest';

import {
  TestCaseResultResultSchema,
  TestCaseResultSchema,
  TestCaseResultSourceSchema,
  UpsertTestCaseResultInputSchema,
} from './test-case-result.schema';

const UUID_A = '123e4567-e89b-12d3-a456-426614174000';
const UUID_B = '123e4567-e89b-12d3-a456-426614174001';
const UUID_C = '123e4567-e89b-12d3-a456-426614174002';
const ISO_TIMESTAMP = '2026-06-08T12:00:00.000Z';

describe('TestCaseResultResultSchema', () => {
  it('accepts the four serde result literals', () => {
    for (const result of ['pass', 'fail', 'blocked', 'not_run'] as const) {
      expect(TestCaseResultResultSchema.parse(result)).toBe(result);
    }
  });

  it('rejects an unknown result', () => {
    expect(TestCaseResultResultSchema.safeParse('skipped').success).toBe(false);
  });
});

describe('TestCaseResultSourceSchema', () => {
  it('accepts the two serde source literals', () => {
    expect(TestCaseResultSourceSchema.parse('manual')).toBe('manual');
    expect(TestCaseResultSourceSchema.parse('sandbox')).toBe('sandbox');
  });

  it('rejects an unknown source', () => {
    expect(TestCaseResultSourceSchema.safeParse('import').success).toBe(false);
  });
});

describe('TestCaseResultSchema', () => {
  it('round-trips a sandbox-sourced failing row with null manual fields', () => {
    const parsed = TestCaseResultSchema.parse({
      id: UUID_A,
      artifactId: UUID_B,
      caseId: 'TC-LOGIN-01',
      actualOutput: 'expected 401, got 500',
      result: 'fail',
      remarks: null,
      source: 'sandbox',
      runId: UUID_C,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expect(parsed.source).toBe('sandbox');
    expect(parsed.remarks).toBeNull();
    expect(parsed.runId).toBe(UUID_C);
  });

  it('accepts a manual not-run row with null run id (serde None → null)', () => {
    const parsed = TestCaseResultSchema.parse({
      id: UUID_A,
      artifactId: UUID_B,
      caseId: 'TC-EDGE-7',
      actualOutput: null,
      result: 'not_run',
      remarks: 'awaiting tester',
      source: 'manual',
      runId: null,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expect(parsed.runId).toBeNull();
    expect(parsed.actualOutput).toBeNull();
  });

  it('rejects an omitted nullable column — serde always serializes it', () => {
    expect(
      TestCaseResultSchema.safeParse({
        id: UUID_A,
        artifactId: UUID_B,
        caseId: 'TC-A',
        // actualOutput intentionally omitted
        result: 'pass',
        remarks: null,
        source: 'sandbox',
        runId: null,
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });

  it('rejects a non-uuid artifactId', () => {
    expect(
      TestCaseResultSchema.safeParse({
        id: UUID_A,
        artifactId: 'nope',
        caseId: 'TC-A',
        actualOutput: null,
        result: 'pass',
        remarks: null,
        source: 'manual',
        runId: null,
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });
});

describe('UpsertTestCaseResultInputSchema', () => {
  it('accepts a minimal manual upsert with optional fields omitted', () => {
    const parsed = UpsertTestCaseResultInputSchema.parse({
      artifactId: UUID_A,
      caseId: 'TC-LOGIN-01',
      result: 'blocked',
    });
    expect(parsed.result).toBe('blocked');
    expect(parsed.actualOutput).toBeUndefined();
    expect(parsed.remarks).toBeUndefined();
  });

  it('carries actual output and remarks when present', () => {
    const parsed = UpsertTestCaseResultInputSchema.parse({
      artifactId: UUID_A,
      caseId: 'TC-LOGIN-01',
      actualOutput: 'redirected to /home',
      result: 'pass',
      remarks: 'verified on Firefox',
    });
    expect(parsed.actualOutput).toBe('redirected to /home');
  });

  it('rejects an upsert missing the required result', () => {
    expect(
      UpsertTestCaseResultInputSchema.safeParse({
        artifactId: UUID_A,
        caseId: 'TC-A',
      }).success,
    ).toBe(false);
  });
});
