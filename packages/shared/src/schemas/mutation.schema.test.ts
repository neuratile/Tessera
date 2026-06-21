import { describe, expect, it } from 'vitest';

import {
  ImproveOutcomeSchema,
  ImproveRequestSchema,
  ImproveResultSchema,
  ImproveStreamEventSchema,
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

describe('ImproveOutcomeSchema', () => {
  it('accepts the five serde outcome literals', () => {
    for (const outcome of ['improved', 'perfect', 'exhausted', 'no_progress', 'error'] as const) {
      expect(ImproveOutcomeSchema.parse(outcome)).toBe(outcome);
    }
  });

  it('rejects an unknown outcome', () => {
    expect(ImproveOutcomeSchema.safeParse('healed').success).toBe(false);
  });
});

describe('ImproveResultSchema', () => {
  it('round-trips an improved result with its attempt trail', () => {
    const parsed = ImproveResultSchema.parse({
      outcome: 'improved',
      attemptsUsed: 2,
      finalArtifactId: 'a-2',
      startScore: 0.5,
      finalScore: 0.9,
      attempts: [
        { attempt: 1, artifactId: 'a-1', score: 0.5, killed: 5, survived: 5 },
        { attempt: 2, artifactId: 'a-2', score: 0.9, killed: 9, survived: 1 },
      ],
    });
    expect(parsed.outcome).toBe('improved');
    expect(parsed.startScore).toBeCloseTo(0.5);
    expect(parsed.finalScore).toBeCloseTo(0.9);
    expect(parsed.attempts).toHaveLength(2);
    expect(parsed.errorMessage).toBeUndefined();
  });

  it('carries an errorMessage on an error outcome', () => {
    const parsed = ImproveResultSchema.parse({
      outcome: 'error',
      attemptsUsed: 1,
      finalArtifactId: 'a-1',
      startScore: 0,
      finalScore: 0,
      attempts: [{ attempt: 1, artifactId: 'a-1', score: 0, killed: 0, survived: 1 }],
      errorMessage: 'Regenerating the test cases failed on attempt 1: boom',
    });
    expect(parsed.errorMessage).toContain('Regenerating');
  });

  it('rejects a score outside [0, 1]', () => {
    expect(
      ImproveResultSchema.safeParse({
        outcome: 'perfect',
        attemptsUsed: 1,
        finalArtifactId: 'a-1',
        startScore: 0,
        finalScore: 1.4,
        attempts: [],
      }).success,
    ).toBe(false);
  });
});

describe('ImproveRequestSchema', () => {
  it('round-trips a request with the optional defaults omitted', () => {
    const parsed = ImproveRequestSchema.parse({
      artifactId: 'a-1',
      maxAttempts: 3,
      maxMutants: 40,
      optInConfirmed: true,
      model: 'qwen2.5-coder:7b',
      provider: 'ollama',
      projectId: 'p-1',
      projectName: 'demo',
    });
    expect(parsed.maxMutants).toBe(40);
    expect(parsed.clientRunId).toBeUndefined();
  });

  it('rejects a non-positive maxAttempts', () => {
    expect(
      ImproveRequestSchema.safeParse({
        artifactId: 'a-1',
        maxAttempts: 0,
        maxMutants: 40,
        optInConfirmed: true,
        model: 'm',
        provider: 'ollama',
        projectId: 'p-1',
        projectName: 'demo',
      }).success,
    ).toBe(false);
  });
});

describe('ImproveStreamEventSchema', () => {
  it('round-trips a per-attempt progress event', () => {
    const parsed = ImproveStreamEventSchema.parse({
      improveId: 'i-1',
      kind: 'attempt',
      attempt: 2,
      score: 0.93,
    });
    expect(parsed.attempt).toBe(2);
    expect(parsed.score).toBeCloseTo(0.93);
  });
});
