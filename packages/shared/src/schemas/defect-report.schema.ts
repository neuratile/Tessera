import { z } from 'zod';

export const DefectSeveritySchema = z.union([
  z.literal('critical'),
  z.literal('major'),
  z.literal('minor'),
  z.literal('trivial'),
]);

export type DefectSeverity = z.infer<typeof DefectSeveritySchema>;

/**
 * CWE top-class alignment (`defect_report_v2`): input_validation
 * (CWE-20), auth (CWE-287/862), resource_management (CWE-400/401),
 * logic (CWE-840), error_handling (CWE-755), concurrency (CWE-362).
 */
export const DefectCategorySchema = z.union([
  z.literal('input_validation'),
  z.literal('auth'),
  z.literal('resource_management'),
  z.literal('logic'),
  z.literal('error_handling'),
  z.literal('concurrency'),
]);

export type DefectCategory = z.infer<typeof DefectCategorySchema>;

export const DefectConfidenceSchema = z.union([z.literal('high'), z.literal('medium')]);

export type DefectConfidence = z.infer<typeof DefectConfidenceSchema>;

/**
 * Source location of one finding — mirrors the `location` object in the
 * Rust `emit_defect_report` v2 tool schema (`prompts/defect_report_v2.rs`).
 */
export const DefectLocationSchema = z
  .object({
    symbol: z.string().min(1),
    startLine: z.number().int().min(1),
    endLine: z.number().int().min(1),
    fileHint: z.string().optional(),
  })
  .refine((location) => location.endLine >= location.startLine, {
    message: 'endLine must be greater than or equal to startLine',
    path: ['endLine'],
  });

export type DefectLocation = z.infer<typeof DefectLocationSchema>;

/**
 * Structured payload for a defect report artifact (`structured_data` JSON).
 *
 * Mirrors the Rust `emit_defect_report` tool schema in
 * `prompts/defect_report_v2.rs` per rules.md §12.3.1 — replaces the old
 * loose shape (string `location`, free-text `category`) with the full
 * v2 mirror: id, confidence, impact, required `fixSuggestion`, and
 * evidence fields at parity with the bug report.
 */
export const DefectReportSchema = z.object({
  findings: z.array(
    z.object({
      id: z.string().regex(/^DEF-[A-Z0-9_-]+$/),
      severity: DefectSeveritySchema,
      category: DefectCategorySchema,
      confidence: DefectConfidenceSchema,
      location: DefectLocationSchema,
      description: z.string().min(10),
      impact: z.string().min(5),
      fixSuggestion: z.string().min(5),
      evidenceSnippet: z.string().optional(),
    }),
  ),
  summary: z.string().optional(),
});

export type DefectReport = z.infer<typeof DefectReportSchema>;
