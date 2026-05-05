import { z } from 'zod';

import { LlmProviderIdSchema } from './llm-provider.schema';

/**
 * Artifact-type literals — mirror `ArtifactType` in
 * `apps/desktop/src-tauri/src/repositories/artifact_repo.rs` (kebab-case
 * over the IPC boundary).
 */
export const GenerationArtifactTypeSchema = z.union([
  z.literal('context-md'),
  z.literal('test-plan'),
  z.literal('test-cases'),
  z.literal('defect-report'),
  z.literal('bug-report'),
]);

export type GenerationArtifactType = z.infer<typeof GenerationArtifactTypeSchema>;

/**
 * Arguments accepted by the `generate_artifact` IPC command — mirrors
 * `GenerateArgs` in `commands/generation.rs`. Optional fields default to
 * empty strings server-side and may be omitted by callers.
 */
export const GenerateArgsSchema = z.object({
  projectId: z.string().uuid(),
  projectName: z.string().min(1),
  artifactType: GenerationArtifactTypeSchema,
  model: z.string().min(1),
  provider: LlmProviderIdSchema,
  scopeHint: z.string().optional(),
  projectSummary: z.string().optional(),
  reviewerFeedback: z.string().optional(),
  parentId: z.string().uuid().optional(),
});

export type GenerateArgs = z.infer<typeof GenerateArgsSchema>;

/**
 * Response returned by `generate_artifact` — mirrors `GenerateResponse`
 * in `commands/generation.rs`.
 */
export const GenerateResponseSchema = z.object({
  artifactId: z.string().uuid(),
  artifactType: GenerationArtifactTypeSchema,
  contentMd: z.string(),
  usageInputTokens: z.number().int().nonnegative(),
  usageOutputTokens: z.number().int().nonnegative(),
});

export type GenerateResponse = z.infer<typeof GenerateResponseSchema>;
