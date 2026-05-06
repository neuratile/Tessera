import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { readDir } from '@tauri-apps/plugin-fs';

import type { FsEntry } from '@/stores/workspace-store';

import { IpcError, asMessage } from './error';

/**
 * Tauri filesystem wrappers for the workspace shell.
 *
 * `webkitdirectory` is intentionally avoided — that browser API loads
 * every file into memory and gives no real disk path. Tauri's
 * `dialog::open` returns the absolute disk path; the backend then walks
 * the directory via `fs::readDir`. This matches the Phase 6 contract
 * where `create_project` accepts a `rootPath` string.
 */

/** Open a native folder picker. Returns the absolute path, or `null` if
 *  the user cancelled. */
export async function pickFolder(): Promise<string | null> {
  try {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: 'Open project folder',
    });
    if (selected === null) return null;
    if (Array.isArray(selected)) {
      const first: unknown = selected[0];
      return typeof first === 'string' ? first : null;
    }
    return typeof selected === 'string' ? selected : null;
  } catch (err) {
    throw new IpcError('dialog.open', asMessage(err), { cause: err });
  }
}

/** Hidden / dependency / build dirs we never list in the explorer. The
 *  backend `file_discovery_service` already filters these for analysis;
 *  the UI does the same so the tree stays readable. */
const SKIP_DIRECTORIES = new Set<string>([
  '.git',
  '.svn',
  '.hg',
  '.idea',
  '.vscode',
  'node_modules',
  '.pnpm-store',
  'target',
  'dist',
  'build',
  '.next',
  '.turbo',
  '__pycache__',
]);

/**
 * Read the immediate children of `absolutePath` and convert them to the
 * shape `react-arborist` consumes. Directories are returned with
 * `children: []` so the tree component knows they are expandable —
 * the actual contents are fetched lazily on expand.
 */
export async function readDirectoryEntries(
  absolutePath: string,
  relativePrefix: string,
): Promise<FsEntry[]> {
  let entries: Awaited<ReturnType<typeof readDir>>;
  try {
    entries = await readDir(absolutePath);
  } catch (err) {
    throw new IpcError('fs.readDir', asMessage(err), { cause: err });
  }

  const out: FsEntry[] = [];
  for (const entry of entries) {
    if (entry.name === undefined || entry.name === '') continue;
    if (entry.name.startsWith('.') && SKIP_DIRECTORIES.has(entry.name)) continue;
    if (SKIP_DIRECTORIES.has(entry.name)) continue;

    const relativePath =
      relativePrefix === '' ? entry.name : `${relativePrefix}/${entry.name}`;
    const childAbsolutePath = joinPath(absolutePath, entry.name);
    const kind: 'file' | 'directory' = entry.isDirectory === true ? 'directory' : 'file';

    const built: FsEntry =
      kind === 'directory'
        ? {
            id: relativePath,
            name: entry.name,
            relativePath,
            absolutePath: childAbsolutePath,
            kind,
            // Empty array signals "expandable but unloaded" to react-arborist.
            children: [],
          }
        : {
            id: relativePath,
            name: entry.name,
            relativePath,
            absolutePath: childAbsolutePath,
            kind,
          };
    out.push(built);
  }

  // Directories first, then files; alphabetical within each group.
  out.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'directory' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  return out;
}

/**
 * Cross-platform path join — Tauri's plugin-fs returns paths with the
 * separator the host OS uses, so we mirror that. Windows accepts
 * forward slashes for most APIs but mixing is ugly; detect here.
 */
function joinPath(parent: string, child: string): string {
  const sep = parent.includes('\\') && !parent.includes('/') ? '\\' : '/';
  if (parent.endsWith('/') || parent.endsWith('\\')) return `${parent}${child}`;
  return `${parent}${sep}${child}`;
}
