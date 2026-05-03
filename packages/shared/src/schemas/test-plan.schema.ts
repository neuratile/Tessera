import { z } from 'zod';

/**
 * Structured payload for a test plan artifact (`structured_data` JSON).
 */
export const TestPlanSchema = z.object({
  summary: z.string(),
  objectives: z.array(z.string()),
  scopeIn: z.array(z.string()),
  scopeOut: z.array(z.string()),
  strategy: z.string(),
  environments: z.array(z.string()),
  risks: z.array(
    z.object({
      description: z.string(),
      mitigation: z.string().optional(),
    }),
  ),
  entryCriteria: z.array(z.string()),
  exitCriteria: z.array(z.string()),
});

export type TestPlan = z.infer<typeof TestPlanSchema>;
