import { z } from 'zod';

/**
 * Test levels (`test_plan_v2`) — 29119-lite ladder.
 */
export const TestLevelSchema = z.union([
  z.literal('unit'),
  z.literal('integration'),
  z.literal('system'),
  z.literal('e2e'),
  z.literal('acceptance'),
]);

export type TestLevel = z.infer<typeof TestLevelSchema>;

export const TestTypeSchema = z.union([
  z.literal('functional'),
  z.literal('performance'),
  z.literal('security'),
  z.literal('usability'),
  z.literal('reliability'),
  z.literal('compatibility'),
  z.literal('regression'),
]);

export type TestType = z.infer<typeof TestTypeSchema>;

/**
 * Structured payload for a test plan artifact (`structured_data` JSON).
 *
 * Mirrors the Rust `emit_test_plan` tool schema in
 * `prompts/test_plan_v2.rs` (rules.md §12.3.1): nested
 * `scope { inScope, outOfScope }` replaces v1's flat scopeIn/scopeOut;
 * adds suspension criteria, test levels/types, and deliverables.
 */
export const TestPlanSchema = z.object({
  summary: z.string().min(1).max(1500),
  objectives: z.array(z.string().min(1)),
  scope: z.object({
    inScope: z.array(z.string().min(1)),
    outOfScope: z.array(z.string().min(1)),
  }),
  strategy: z.string().min(1).max(2000),
  testLevels: z.array(TestLevelSchema),
  testTypes: z.array(TestTypeSchema),
  environments: z.array(z.string().min(1)),
  risks: z.array(
    z.object({
      description: z.string().min(1),
      mitigation: z.string().min(1).optional(),
    }),
  ),
  entryCriteria: z.array(z.string().min(1)),
  exitCriteria: z.array(z.string().min(1)),
  suspensionCriteria: z.array(z.string().min(1)),
  deliverables: z.array(z.string().min(1)),
});

export type TestPlan = z.infer<typeof TestPlanSchema>;
