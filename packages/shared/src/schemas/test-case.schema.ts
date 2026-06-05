import { z } from 'zod';

export const TestCasePrioritySchema = z.union([
  z.literal('p0'),
  z.literal('p1'),
  z.literal('p2'),
  z.literal('p3'),
]);

export type TestCasePriority = z.infer<typeof TestCasePrioritySchema>;

/**
 * One file in the runnable workspace carried on a test-cases artifact —
 * mirrors the Rust `WorkspaceFile` the sandbox runner consumes
 * (`structured_data.files[]`). `isTest` is true for a generated vitest
 * spec, false for source-under-test.
 */
export const TestCaseFileSchema = z.object({
  path: z.string().min(1),
  contents: z.string(),
  isTest: z.boolean(),
});

export type TestCaseFile = z.infer<typeof TestCaseFileSchema>;

/**
 * Structured payload for test cases artifact (`structured_data` JSON).
 *
 * `files` is the optional runnable workspace (source-under-test + vitest
 * specs) the sandbox test runner executes; descriptive-only artifacts omit
 * it. Mirrors the optional `files` array in the Rust `emit_test_cases` tool
 * schema (`prompts/test_cases_v1.rs`).
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
  files: z.array(TestCaseFileSchema).optional(),
});

export type TestCase = z.infer<typeof TestCaseSchema>;
