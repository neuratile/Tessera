import { save } from '@tauri-apps/plugin-dialog';
import type { ExportFormat, ExportOutcome } from '@testing-ide/shared';

import { IpcError, asMessage } from './ipc/error';
import { exportArtifact as exportArtifactIpc } from './ipc/exports';

/**
 * Slug an artifact title into a stable filename with the given
 * extension. Generalizes the markdown-only helper that previously
 * lived in `export-markdown.ts` so every export format shares one
 * slug rule.
 */
export function buildExportFilename(title: string, extension: string): string {
  const trimmed = title.trim();
  const normalized = trimmed.length > 0 ? trimmed : 'artifact';
  const slug = normalized
    .toLowerCase()
    .replace(/[^a-z0-9]+/gu, '-')
    .replace(/^-+|-+$/gu, '')
    .slice(0, 80);

  return `${slug.length > 0 ? slug : 'artifact'}.${extension}`;
}

const FORMAT_DIALOG: Record<ExportFormat, { title: string; filterName: string }> = {
  xlsx: { title: 'Export Excel workbook', filterName: 'Excel Workbook' },
  csv: { title: 'Export CSV', filterName: 'CSV' },
  tsv: { title: 'Export TSV', filterName: 'TSV' },
};

/**
 * Full export flow for structured artifact data: ask the user for a
 * destination via the save dialog, then let the Rust export service
 * map + write the file(s). Returns `null` when the user cancels the
 * dialog, otherwise the list of files written.
 */
export async function exportArtifactToFile(
  artifactId: string,
  title: string,
  format: ExportFormat,
): Promise<ExportOutcome | null> {
  const dialog = FORMAT_DIALOG[format];

  let selectedPath: string | null;
  try {
    selectedPath = await save({
      title: dialog.title,
      defaultPath: buildExportFilename(title, format),
      filters: [{ name: dialog.filterName, extensions: [format] }],
    });
  } catch (error) {
    throw new IpcError('dialog.save', asMessage(error), { cause: error });
  }

  if (selectedPath === null) {
    return null;
  }

  return exportArtifactIpc(artifactId, format, selectedPath);
}
