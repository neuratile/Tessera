import { z } from 'zod';

/**
 * 5-level impact scale (`bug_report_v2`). Independent of `priority`:
 * severity is impact on the system, priority is urgency of the fix.
 */
export const BugSeveritySchema = z.union([
  z.literal('blocker'),
  z.literal('critical'),
  z.literal('major'),
  z.literal('minor'),
  z.literal('trivial'),
]);

export type BugSeverity = z.infer<typeof BugSeveritySchema>;

export const BugPrioritySchema = z.union([
  z.literal('p0'),
  z.literal('p1'),
  z.literal('p2'),
  z.literal('p3'),
]);

export type BugPriority = z.infer<typeof BugPrioritySchema>;

export const BugReproducibilitySchema = z.union([
  z.literal('always'),
  z.literal('intermittent'),
  z.literal('once'),
]);

export type BugReproducibility = z.infer<typeof BugReproducibilitySchema>;

/**
 * Root-cause analysis block — mirrors the `rootCause` object in the Rust
 * `emit_bug_report` v2 tool schema (`prompts/bug_report_v2.rs`).
 */
export const BugRootCauseSchema = z
  .object({
    symbol: z.string().min(1),
    startLine: z.number().int().min(1).optional(),
    endLine: z.number().int().min(1).optional(),
    fileHint: z.string().optional(),
    explanation: z.string().min(10),
  })
  .refine(
    (cause) =>
      cause.startLine === undefined ||
      cause.endLine === undefined ||
      cause.endLine >= cause.startLine,
    {
      message: 'endLine must be greater than or equal to startLine',
      path: ['endLine'],
    },
  );

export type BugRootCause = z.infer<typeof BugRootCauseSchema>;

/**
 * Structured payload for a bug-report artifact (`structured_data` JSON).
 *
 * Mirrors the Rust `emit_bug_report` tool schema in
 * `prompts/bug_report_v2.rs` per rules.md §12.3.1 — closes the
 * free-form-JSON gap bug reports had before v2.
 */
export const BugReportSchema = z.object({
  bugs: z.array(
    z.object({
      id: z.string().regex(/^BUG-[A-Z0-9_-]+$/),
      title: z.string().min(10).max(200),
      severity: BugSeveritySchema,
      priority: BugPrioritySchema,
      reproducibility: BugReproducibilitySchema,
      environment: z.string().optional(),
      component: z.string().optional(),
      stepsToReproduce: z.array(z.string().min(1)).min(1),
      expectedBehavior: z.string().min(1),
      actualBehavior: z.string().min(1),
      workaround: z.string().optional(),
      rootCause: BugRootCauseSchema,
      evidenceSnippet: z.string().optional(),
    }),
  ),
});

export type BugReport = z.infer<typeof BugReportSchema>;
