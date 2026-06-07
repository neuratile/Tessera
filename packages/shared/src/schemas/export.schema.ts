import { z } from 'zod';

/**
 * Output formats accepted by the `export_artifact` command — mirrors
 * `ExportFormat` in `apps/desktop/src-tauri/src/services/export/mod.rs`
 * (lowercase serde wire values).
 */
export const ExportFormatSchema = z.union([
  z.literal('xlsx'),
  z.literal('csv'),
  z.literal('tsv'),
]);

export type ExportFormat = z.infer<typeof ExportFormatSchema>;

/**
 * Result of a file export — mirrors `ExportOutcome` in
 * `apps/desktop/src-tauri/src/commands/exports.rs`. CSV/TSV exports of
 * multi-section artifacts write sibling files, so `files` lists every
 * path written (always at least the user-chosen one).
 */
export const ExportOutcomeSchema = z.object({
  files: z.array(z.string().min(1)).min(1),
});

export type ExportOutcome = z.infer<typeof ExportOutcomeSchema>;
