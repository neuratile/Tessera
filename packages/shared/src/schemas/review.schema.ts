import { z } from 'zod';
import { LlmProviderIdSchema } from './llm-provider.schema';

// Mirrors repositories/review_repo/types.rs. No credentials or raw source on the wire.
const uuid = z.string().uuid();
const uint = z.number().int().min(0).max(0xffffffff);
const text = z.string().refine((s) => s.trim().length > 0 && new TextEncoder().encode(s).length <= 16384 && !s.includes('\0'));
const hash = z.string().regex(/^[0-9a-f]{64}$/);
const path = z.string().refine((s) => s.length > 0 && new TextEncoder().encode(s).length <= 4096 &&
  !/[\0\\:]/.test(s) && s.split('/').length <= 64 && s.split('/').every((p) => p !== '' && p !== '.' && p !== '..'));
const timestamp = z.string().datetime({ offset: true });
export const ReviewStatusSchema = z.enum(['queued', 'running', 'completed', 'failed', 'cancelled', 'interrupted']);
export const EvidenceStateSchema = z.enum(['suspected', 'reproduced', 'inconclusive']);
export const ReviewProvenanceSchema = z.object({
  providerConfigId: uuid, provider: LlmProviderIdSchema, model: text, promptVersion: text,
  inputTokens: uint.optional(), outputTokens: uint.optional(),
}).strict();
const error = z.object({ code: text, message: text }).strict();
const source = z.object({
  path, side: z.enum(['old', 'new']), blobId: z.string(), mode: z.enum(['100644', '100755']),
  contentHash: hash, byteLength: uint.max(128 * 1024), lineCount: uint,
}).strict().refine((s) => s.lineCount <= s.byteLength);
const change = z.object({
  changeKind: z.enum(['added', 'modified', 'deleted', 'renamed']), oldPath: path.optional(), newPath: path.optional(),
}).strict().refine((c) => {
  switch (c.changeKind) {
    case 'added': return c.oldPath === undefined && c.newPath !== undefined;
    case 'deleted': return c.oldPath !== undefined && c.newPath === undefined;
    case 'modified': return c.oldPath !== undefined && c.oldPath === c.newPath;
    case 'renamed': return c.oldPath !== undefined && c.newPath !== undefined && c.oldPath !== c.newPath;
  }
});
export const ReviewSnapshotSchema = z.object({
  snapshotId: uuid, gitObjectFormat: z.enum(['sha1', 'sha256']), baseCommit: z.string().optional(), indexFingerprint: hash,
  sources: z.array(source), changes: z.array(change).max(50), contextPaths: z.array(path).max(20),
  exclusions: z.array(z.object({ path: text, reason: text }).strict()),
}).strict().superRefine((s, ctx) => {
  const hex = new RegExp(`^[0-9a-f]{${s.gitObjectFormat === 'sha1' ? 40 : 64}}$`);
  const keys = new Set(s.sources.map((b) => `${b.side}:${b.path}`));
  const used: string[] = [];
  s.changes.forEach((c) => {
    if (c.oldPath !== undefined) used.push(`old:${c.oldPath}`);
    if (c.newPath !== undefined) used.push(`new:${c.newPath}`);
  });
  used.push(...s.contextPaths.map((p) => `new:${p}`));
  const contents = new Map<string, typeof s.sources[number]>();
  const blobHashes = new Map<string, string>();
  let size = 0;
  let inconsistentContent = false;
  for (const blob of s.sources) {
    const previous = contents.get(blob.contentHash);
    if (previous === undefined) {
      contents.set(blob.contentHash, blob);
      size += blob.byteLength;
    } else if (previous.blobId !== blob.blobId || previous.byteLength !== blob.byteLength || previous.lineCount !== blob.lineCount) {
      inconsistentContent = true;
    }
    const previousHash = blobHashes.get(blob.blobId);
    if (previousHash !== undefined && previousHash !== blob.contentHash) inconsistentContent = true;
    blobHashes.set(blob.blobId, blob.contentHash);
  }
  if ((s.baseCommit !== undefined && !hex.test(s.baseCommit)) || s.sources.some((b) => !hex.test(b.blobId)) ||
    keys.size !== s.sources.length || new Set(used).size !== used.length || used.length !== keys.size || used.some((k) => !keys.has(k)) ||
    inconsistentContent || size > 1024 * 1024) {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: 'Invalid snapshot manifest identity' });
  }
});
export const ReviewSourceReferenceSchema = z.object({
  snapshotId: uuid, path, side: z.enum(['old', 'new']), lineStart: uint.optional(), lineEnd: uint.optional(),
}).strict().refine((s) => (s.lineStart === undefined && s.lineEnd === undefined) ||
  (s.lineStart !== undefined && s.lineEnd !== undefined && s.lineStart > 0 && s.lineEnd >= s.lineStart));
export const ReviewFindingSchema = z.object({
  findingId: uuid, summary: text, explanation: text, expectedBehavior: text,
  severity: z.enum(['critical', 'major', 'minor', 'trivial']), sources: z.array(ReviewSourceReferenceSchema).min(1),
  evidenceState: EvidenceStateSchema,
}).strict();
export const ReviewStepSchema = z.object({
  stepId: uuid, step: z.enum(['capture', 'context', 'findings', 'persist']), status: ReviewStatusSchema,
  progress: uint.max(100), createdAt: timestamp, error: error.optional(),
}).strict();
export const ReviewSessionSchema = z.object({
  reviewId: uuid, projectId: uuid, scope: z.literal('staged'), status: ReviewStatusSchema,
  sequence: uint, progress: uint.max(100), provenance: ReviewProvenanceSchema,
  createdAt: timestamp, updatedAt: timestamp,
  outcome: z.enum(['reviewed', 'no_changes', 'nothing_reviewable']).optional(), error: error.optional(),
  snapshot: ReviewSnapshotSchema.optional(), steps: z.array(ReviewStepSchema), findings: z.array(ReviewFindingSchema),
  truncation: z.array(z.object({ resource: text, reason: text, omittedCount: uint.min(1) }).strict()),
}).strict().superRefine((r, ctx) => {
  const fail = () => ctx.addIssue({ code: z.ZodIssueCode.custom, message: 'Inconsistent review lifecycle or source reference' });
  if ((r.status === 'completed') !== (r.outcome !== undefined) ||
    (r.status === 'completed' && (r.progress !== 100 || r.snapshot === undefined)) ||
    (r.status === 'failed' && r.error === undefined) ||
    (r.outcome !== undefined && r.outcome !== 'reviewed' && r.findings.length > 0) ||
    new Set(r.steps.map((s) => s.stepId)).size !== r.steps.length ||
    new Set(r.findings.map((f) => f.findingId)).size !== r.findings.length) fail();
  r.findings.forEach((f) => f.sources.forEach((ref) => {
    const blob = r.snapshot?.sources.find((b) => b.path === ref.path && b.side === ref.side);
    if (ref.snapshotId !== r.snapshot?.snapshotId || blob === undefined ||
      (ref.lineEnd !== undefined && ref.lineEnd > blob.lineCount)) fail();
  }));
});
export const ReviewArtifactLinkSchema = z.object({
  linkId: uuid, findingId: uuid, snapshotId: uuid, artifactId: uuid, testId: uuid,
  testVersion: uint.min(1), testContentHash: hash, provenance: ReviewProvenanceSchema,
}).strict();
const environment = z.object({
  runner: text, imageId: z.string().regex(/^sha256:[0-9a-f]{64}$/), startedAt: timestamp, finishedAt: timestamp,
  timeoutMs: uint.min(1), memoryBytes: uint.min(1), cpuMillis: uint.min(1), outputTruncated: z.boolean(),
}).strict();
export const ReviewRunLinkSchema = z.object({
  attemptId: uuid, artifactLinkId: uuid, runId: uuid, environment, evidenceState: EvidenceStateSchema,
  assertion: text.optional(), expected: text.optional(), observed: text.optional(),
}).strict().refine((r) => r.evidenceState !== 'suspected' && (r.evidenceState !== 'reproduced' ||
  (!r.environment.outputTruncated && r.assertion !== undefined && r.expected !== undefined && r.observed !== undefined)));

export type ReviewSession = z.infer<typeof ReviewSessionSchema>;
export type ReviewSnapshot = z.infer<typeof ReviewSnapshotSchema>;
export type ReviewSourceReference = z.infer<typeof ReviewSourceReferenceSchema>;
export type ReviewFinding = z.infer<typeof ReviewFindingSchema>;
export type ReviewProvenance = z.infer<typeof ReviewProvenanceSchema>;
export type ReviewStep = z.infer<typeof ReviewStepSchema>;
export type ReviewArtifactLink = z.infer<typeof ReviewArtifactLinkSchema>;
export type ReviewRunLink = z.infer<typeof ReviewRunLinkSchema>;
