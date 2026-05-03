import { z } from 'zod';

import { DefectReportSchema } from './defect-report.schema';
import { TestCaseSchema } from './test-case.schema';
import { TestPlanSchema } from './test-plan.schema';

const IsoDateTimeSchema = z.string().datetime({ offset: true });

export const ArtifactTypeSchema = z.union([
  z.literal('test-plan'),
  z.literal('test-cases'),
  z.literal('defect-report'),
  z.literal('bug-report'),
]);

export type ArtifactType = z.infer<typeof ArtifactTypeSchema>;

export const ArtifactStatusSchema = z.union([
  z.literal('draft'),
  z.literal('pending_review'),
  z.literal('approved'),
  z.literal('rejected'),
]);

export type ArtifactStatus = z.infer<typeof ArtifactStatusSchema>;

export const StructuredDataSchema = z.union([
  TestPlanSchema,
  TestCaseSchema,
  DefectReportSchema,
  z.record(z.string(), z.unknown()),
]);

/**
 * Artifact row / API resource. `structuredData` must match `type` at the application layer;
 * use narrow helpers after parsing when needed.
 */
export const ArtifactSchema = z.object({
  id: z.string().uuid(),
  projectId: z.string().uuid(),
  type: ArtifactTypeSchema,
  title: z.string().min(1),
  content: z.string(),
  structuredData: StructuredDataSchema,
  status: ArtifactStatusSchema,
  version: z.number().int().positive(),
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export type Artifact = z.infer<typeof ArtifactSchema>;
