import { describe, expect, it } from 'vitest';
import fixture from './fixtures/review.json';
import invalid from './fixtures/review-invalid.json';
import {
  ReviewSessionSchema, ReviewSnapshotSchema, ReviewSourceReferenceSchema,
  ReviewArtifactLinkSchema, ReviewRunLinkSchema,
} from './review.schema';

function mutate(path: string, value: unknown) {
  const candidate = structuredClone(fixture.session);
  const parts = path.split('.');
  let target = candidate as unknown as Record<string, unknown>;
  for (const part of parts.slice(0, -1)) target = target[part] as Record<string, unknown>;
  target[parts[parts.length - 1]!] = value;
  return candidate;
}

function modeSnapshot(count: number) {
  const source = fixture.session.snapshot.sources[0]!;
  return {
    ...fixture.session.snapshot,
    sources: Array.from({ length: count }, (_, i) => {
      const blob = { ...source, path: `src/file${i}.ts`, blobId: i.toString(16).padStart(40, '0'),
        contentHash: i.toString(16).padStart(64, '0'), byteLength: 120 * 1024 };
      return [{ ...blob, side: 'old' as const }, { ...blob, side: 'new' as const, mode: '100755' }];
    }).flat(),
    changes: Array.from({ length: count }, (_, i) => ({ changeKind: 'modified', oldPath: `src/file${i}.ts`, newPath: `src/file${i}.ts` })),
  };
}

describe('review Rust/Zod shared fixtures', () => {
  it('preserves legacy findings and validates optional suggestedTest', () => {
    expect(ReviewSessionSchema.safeParse(fixture.session).success).toBe(true);
    expect(ReviewSessionSchema.safeParse(mutate('findings.0.suggestedTest', 'Assert expected outcome')).success).toBe(true);
    for (const value of ['', '   ', null, 'x'.repeat(16385), '\0']) {
      expect(ReviewSessionSchema.safeParse(mutate('findings.0.suggestedTest', value)).success).toBe(false);
    }
  });
  it('round-trips additive final omissions and rejects malformed omissions', () => {
    const excludedFiles = [{ path: 'src/support.ts', reason: 'prompt_budget' }];
    const candidate = { ...fixture.session, excludedFiles };
    expect(ReviewSessionSchema.parse(candidate)).toEqual(candidate);
    for (const value of [null, [{ path: 'src/a.ts', reason: '' }]]) {
      expect(ReviewSessionSchema.safeParse({ ...fixture.session, excludedFiles: value }).success).toBe(false);
    }
  });
  it('counts distinct source content, not old/new mode-only references', () => {
    expect(ReviewSnapshotSchema.safeParse(modeSnapshot(5)).success).toBe(true);
    expect(ReviewSnapshotSchema.safeParse(modeSnapshot(9)).success).toBe(false);
  });
  it.each([
    ['byteLength', 120 * 1024 + 1], ['lineCount', 2], ['blobId', 'f'.repeat(40)], ['contentHash', 'f'.repeat(64)],
  ])('rejects repeated content with conflicting %s', (key, value) => {
    const snapshot = modeSnapshot(5);
    Object.assign(snapshot.sources[1]!, { [key]: value });
    expect(ReviewSnapshotSchema.safeParse(snapshot).success).toBe(false);
  });
  it('round-trips the same session and version/run bindings without source or keys', () => {
    expect(ReviewSessionSchema.parse(fixture.session)).toEqual(fixture.session);
    expect(ReviewArtifactLinkSchema.parse(fixture.artifactLink)).toEqual(fixture.artifactLink);
    expect(ReviewRunLinkSchema.parse(fixture.runLink)).toEqual(fixture.runLink);
    expect(JSON.stringify(fixture.session)).not.toContain(fixture.sourceUtf8);
    expect(ReviewSessionSchema.safeParse({ ...fixture.session, apiKey: 'secret' }).success).toBe(false);
  });
  it.each(invalid)('rejects malformed $path', ({ path, value }) => {
    expect(ReviewSessionSchema.safeParse(mutate(path, value)).success).toBe(false);
  });
  it.each(['reviewId', 'projectId', 'provenance', 'steps', 'findings', 'createdAt'])('rejects missing %s', (key) => {
    const candidate: Record<string, unknown> = { ...fixture.session };
    delete candidate[key];
    expect(ReviewSessionSchema.safeParse(candidate).success).toBe(false);
  });
  it('accepts omitted file-level lines, rejects missing pairs and duplicate manifests', () => {
    const ref: Record<string, unknown> = { ...fixture.session.findings[0]!.sources[0]! };
    delete ref.lineStart;
    delete ref.lineEnd;
    expect(ReviewSourceReferenceSchema.safeParse(ref).success).toBe(true);
    expect(ReviewSourceReferenceSchema.safeParse({ ...ref, lineStart: 1 }).success).toBe(false);
    const snapshot = fixture.session.snapshot;
    expect(ReviewSnapshotSchema.safeParse({ ...snapshot, sources: [...snapshot.sources, ...snapshot.sources] }).success).toBe(false);
  });
  it('requires exact test version/hash and reproduction assertion details', () => {
    expect(ReviewArtifactLinkSchema.safeParse({ ...fixture.artifactLink, testVersion: 0 }).success).toBe(false);
    expect(ReviewArtifactLinkSchema.safeParse({ ...fixture.artifactLink, testContentHash: 'bad' }).success).toBe(false);
    const attempt: Record<string, unknown> = { ...fixture.runLink };
    delete attempt.assertion;
    expect(ReviewRunLinkSchema.safeParse(attempt).success).toBe(false);
    expect(ReviewRunLinkSchema.safeParse({ ...fixture.runLink,
      environment: { ...fixture.runLink.environment, outputTruncated: true },
    }).success).toBe(false);
    expect(ReviewRunLinkSchema.safeParse({ ...attempt, evidenceState: 'inconclusive' }).success).toBe(true);
  });
});
