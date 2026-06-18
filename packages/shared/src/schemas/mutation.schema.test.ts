import { describe, expect, it } from 'vitest';

import {
  MutantResultSchema,
  MutantStatusSchema,
  MutationCheckRecordSchema,
  MutationCheckSummarySchema,
  MutationResultSchema,
  MutationStreamEventSchema,
} from './mutation.schema';

describe('MutantStatusSchema', () => {
  it('accepts the three serde status literals', () => {
    for (const status of ['killed', 'survived', 'errored'] as const) {
      expect(MutantStatusSchema.parse(status)).toBe(status);
    }
  });

  it('rejects an unknown status', () => {
    expect(MutantStatusSchema.safeParse('escaped').success).toBe(false);
  });
});

describe('MutantResultSchema', () => {
  it('round-trips a survivor with its file:line and operator swap', () => {
    const parsed = MutantResultSchema.parse({
      mutant: {
        file: 'cart.ts',
        line: 42,
        operatorId: 'relational',
        original: '>',
        replacement: '>=',
        byteStart: 100,
        byteEnd: 101,
      },
      status: 'survived',
    });
    expect(parsed.status).toBe('survived');
    expect(parsed.mutant.original).toBe('>');
    expect(parsed.mutant.replacement).toBe('>=');
  });
});

describe('MutationResultSchema', () => {
  it('round-trips a mixed kill/survive score', () => {
    const parsed = MutationResultSchema.parse({
      score: 0.78,
      killed: 31,
      survived: 9,
      errored: 0,
      total: 40,
      baselineRunId: 'r-1',
      mutants: [
        {
          mutant: {
            file: 'cart.ts',
            line: 42,
            operatorId: 'relational',
            original: '>',
            replacement: '>=',
            byteStart: 0,
            byteEnd: 1,
          },
          status: 'survived',
        },
      ],
      droppedCount: 0,
    });
    expect(parsed.score).toBeCloseTo(0.78);
    expect(parsed.killed).toBe(31);
    expect(parsed.mutants).toHaveLength(1);
  });

  it('rejects a score outside [0, 1]', () => {
    expect(
      MutationResultSchema.safeParse({
        score: 1.5,
        killed: 1,
        survived: 0,
        errored: 0,
        total: 1,
        baselineRunId: 'r-1',
        mutants: [],
        droppedCount: 0,
      }).success,
    ).toBe(false);
  });
});

describe('MutationCheckSummarySchema', () => {
  it('round-trips a history header with the baseline run', () => {
    const parsed = MutationCheckSummarySchema.parse({
      id: '11111111-1111-4111-8111-111111111111',
      baselineRunId: '22222222-2222-4222-8222-222222222222',
      score: 0.9,
      killed: 9,
      survived: 1,
      errored: 0,
      total: 10,
      droppedCount: 3,
      createdAt: '2026-06-18T00:00:00+00:00',
    });
    expect(parsed.score).toBeCloseTo(0.9);
    expect(parsed.droppedCount).toBe(3);
  });

  it('accepts a header with baselineRunId omitted (serde None after a purge)', () => {
    const parsed = MutationCheckSummarySchema.parse({
      id: '11111111-1111-4111-8111-111111111111',
      score: 1,
      killed: 2,
      survived: 0,
      errored: 0,
      total: 2,
      droppedCount: 0,
      createdAt: '2026-06-18T00:00:00+00:00',
    });
    expect(parsed.baselineRunId).toBeUndefined();
  });
});

describe('MutationCheckRecordSchema', () => {
  it('extends the summary with the per-mutant list', () => {
    const parsed = MutationCheckRecordSchema.parse({
      id: '11111111-1111-4111-8111-111111111111',
      score: 0.5,
      killed: 1,
      survived: 1,
      errored: 0,
      total: 2,
      droppedCount: 0,
      createdAt: '2026-06-18T00:00:00+00:00',
      mutants: [
        {
          mutant: { file: 'a.ts', line: 1, operatorId: 'arithmetic', original: '+', replacement: '-', byteStart: 0, byteEnd: 0 },
          status: 'killed',
        },
      ],
    });
    expect(parsed.mutants).toHaveLength(1);
    expect(parsed.mutants[0]?.status).toBe('killed');
  });
});

describe('MutationStreamEventSchema', () => {
  it('round-trips a per-mutant progress event', () => {
    const parsed = MutationStreamEventSchema.parse({
      mutationId: 'm-1',
      kind: 'mutant',
      done: 12,
      total: 40,
    });
    expect(parsed.done).toBe(12);
    expect(parsed.total).toBe(40);
  });
});
