import { z } from 'zod';

export const TestCasePrioritySchema = z.union([
  z.literal('p0'),
  z.literal('p1'),
  z.literal('p2'),
  z.literal('p3'),
]);

export type TestCasePriority = z.infer<typeof TestCasePrioritySchema>;

/**
 * Structured payload for test cases artifact (`structured_data` JSON).
 */
export const TestCaseSchema = z.object({
  cases: z.array(
    z.object({
      id: z.string().min(1),
      title: z.string().min(1),
      preconditions: z.array(z.string()).optional(),
      steps: z.array(z.string()),
      expectedResult: z.string(),
      priority: TestCasePrioritySchema,
      traceability: z.array(z.string()).optional(),
    }),
  ),
});

export type TestCase = z.infer<typeof TestCaseSchema>;
