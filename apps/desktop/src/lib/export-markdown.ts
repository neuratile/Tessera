import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile } from '@tauri-apps/plugin-fs';

import { getTestingIdeWriteTextFile, isTestingIdeE2eEnabled } from './e2e-bridge';
import { buildExportFilename } from './export-artifact';
import { IpcError, asMessage } from './ipc/error';

export function buildMarkdownFilename(title: string): string {
  return buildExportFilename(title, 'md');
}

export async function exportMarkdownDocument(
  title: string,
  content: string,
): Promise<string | null> {
  const defaultPath = buildMarkdownFilename(title);

  let selectedPath: string | null;
  try {
    selectedPath = await save({
      title: 'Export markdown',
      defaultPath,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    });
  } catch (error) {
    throw new IpcError('dialog.save', asMessage(error), { cause: error });
  }

  if (selectedPath === null) {
    return null;
  }

  const writeE2eTextFile = getTestingIdeWriteTextFile();
  if (isTestingIdeE2eEnabled()) {
    if (writeE2eTextFile === null) {
      throw new IpcError('e2e.writeTextFile', 'E2E write bridge is not installed');
    }

    try {
      await writeE2eTextFile(selectedPath, content);
      return selectedPath;
    } catch (error) {
      throw new IpcError('e2e.writeTextFile', asMessage(error), { cause: error });
    }
  }

  try {
    await writeTextFile(selectedPath, content);
  } catch (error) {
    throw new IpcError('fs.writeTextFile', asMessage(error), { cause: error });
  }

  return selectedPath;
}
