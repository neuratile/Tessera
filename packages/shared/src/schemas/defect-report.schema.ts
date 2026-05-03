import { z } from 'zod';

export const DefectSeveritySchema = z.union([
  z.literal('critical'),
  z.literal('major'),
  z.literal('minor'),
  z.literal('trivial'),
]);

export type DefectSeverity = z.infer<typeof DefectSeveritySchema>;

/**
 * Structured payload for a defect report artifact (`structured_data` JSON).
 */
export const DefectReportSchema = z.object({
  findings: z.array(
    z.object({
      severity: DefectSeveritySchema,
      category: z.string().min(1),
      location: z.string().min(1),
      description: z.string(),
      suggestedFix: z.string().optional(),
    }),
  ),
});

export type DefectReport = z.infer<typeof DefectReportSchema>;
