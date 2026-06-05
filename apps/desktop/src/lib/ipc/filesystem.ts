import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { readDir, readTextFile, stat, watch as tauriWatch } from '@tauri-apps/plugin-fs';
import type { UnwatchFn, WatchEvent } from '@tauri-apps/plugin-fs';

import type { FsEntry } from '@/stores/workspace-store';

import { IpcError, asMessage } from './error';

/** Hard cap for in-memory file content. Mirrors backend
 *  `MAX_FILE_SIZE_BYTES` minus headroom; the editor refuses to load
 *  anything larger. */
export const MAX_EDITOR_FILE_BYTES = 2 * 1024 * 1024;

/** Extensions the editor refuses to load — binary blobs that Monaco
 *  cannot meaningfully render. The backend AST pipeline rejects the
 *  same set, so the renderer mirrors that policy. */
const BINARY_EXTENSIONS = new Set<string>([
  'png',
  'jpg',
  'jpeg',
  'gif',
  'webp',
  'ico',
  'bmp',
  'svg',
  'pdf',
  'zip',
  'tar',
  'gz',
  'rar',
  '7z',
  'exe',
  'dll',
  'so',
  'dylib',
  'class',
  'jar',
  'wasm',
  'mp3',
  'mp4',
  'mov',
  'avi',
  'wav',
  'ogg',
  'ttf',
  'otf',
  'woff',
  'woff2',
  'db',
  'sqlite',
]);

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

/** Get the lowercase extension (no leading dot) of a path. Empty string
 *  when the file has no extension. */
export function fileExtension(path: string): string {
  const base = path.split(/[\\/]/u).pop() ?? '';
  const dot = base.lastIndexOf('.');
  if (dot <= 0) return '';
  return base.slice(dot + 1).toLowerCase();
}

/** Whether the editor will attempt to load this file. Used by the file
 *  explorer to grey out unsupported entries before the user clicks. */
export function isLikelyBinary(path: string): boolean {
  return BINARY_EXTENSIONS.has(fileExtension(path));
}

/**
 * Read a file from disk into a UTF-8 string.
 *
 * Refuses files larger than `MAX_EDITOR_FILE_BYTES` and known-binary
 * extensions before touching disk so a misclick on a 200 MB asset
 * does not freeze the renderer.
 */
export async function readFileText(absolutePath: string): Promise<string> {
  if (isLikelyBinary(absolutePath)) {
    throw new IpcError(
      'fs.readTextFile',
      `cannot open binary file (.${fileExtension(absolutePath)}) in the editor`,
    );
  }

  let size: number;
  try {
    const meta = await stat(absolutePath);
    size = meta.size;
  } catch (err) {
    throw new IpcError('fs.stat', asMessage(err), { cause: err });
  }

  if (size > MAX_EDITOR_FILE_BYTES) {
    throw new IpcError(
      'fs.readTextFile',
      `file is too large for the editor (${formatBytes(size)} > ${formatBytes(
        MAX_EDITOR_FILE_BYTES,
      )})`,
    );
  }

  try {
    return await readTextFile(absolutePath);
  } catch (err) {
    throw new IpcError('fs.readTextFile', asMessage(err), { cause: err });
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Watch a directory recursively for any file or directory changes.
 * Returns an unwatch function.
 */
export async function watchDirectory(
  path: string,
  cb: (event: WatchEvent) => void,
  options?: { recursive?: boolean; delayMs?: number },
): Promise<UnwatchFn> {
  try {
    return await tauriWatch(path, cb, options);
  } catch (err) {
    throw new IpcError('fs.watch', asMessage(err), { cause: err });
  }
}

