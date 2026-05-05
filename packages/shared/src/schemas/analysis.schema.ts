import { z } from 'zod';

/**
 * Analysis pipeline outcome — mirrors `AnalysisOutcome` from
 * `apps/desktop/src-tauri/src/services/analysis_service.rs`.
 */
export const AnalysisOutcomeSchema = z.object({
  projectId: z.string().uuid(),
  filesDiscovered: z.number().int().nonnegative(),
  filesParsed: z.number().int().nonnegative(),
  chunksCreated: z.number().int().nonnegative(),
  chunksEmbedded: z.number().int().nonnegative(),
  totalSizeBytes: z.number().int().nonnegative(),
});

export type AnalysisOutcome = z.infer<typeof AnalysisOutcomeSchema>;
