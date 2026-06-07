import {
  type ExportFormat,
  type ExportOutcome,
  ExportOutcomeSchema,
} from '@testing-ide/shared';

import { invokeAndParse, invokeString } from './invoke';

/**
 * Export an artifact's structured data to a file on disk. The Rust
 * side validates `destPath`, maps the payload to the export IR, and
 * writes xlsx/csv/tsv. Returns every file written (CSV/TSV exports of
 * multi-section artifacts emit siblings).
 */
export async function exportArtifact(
  artifactId: string,
  format: ExportFormat,
  destPath: string,
): Promise<ExportOutcome> {
  return invokeAndParse('export_artifact', ExportOutcomeSchema, {
    artifactId,
    format,
    destPath,
  });
}

/**
 * Render an artifact as clipboard-ready TSV. The TSV is always built
 * Rust-side so the artifact→table mapping logic never duplicates in
 * TypeScript.
 */
export async function getArtifactTsv(artifactId: string): Promise<string> {
  return invokeString('get_artifact_tsv', { artifactId });
}
