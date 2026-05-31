import { filesystem, getErrorMessage } from '@/lib/ipc';
import { useEditorStore, type EditorTab } from '@/stores/editor-store';
import type { FsEntry } from '@/stores/workspace-store';

/**
 * Open a file entry in the editor. Idempotent: a second call for the
 * same path activates the existing tab without re-reading disk. The
 * read happens through `filesystem.readFileText` which guards against
 * binary files and oversize payloads.
 */
export function openFileInEditor(entry: FsEntry): void {
  if (entry.kind !== 'file') return;

  const store = useEditorStore.getState();
  const tab: EditorTab = {
    id: entry.relativePath,
    relativePath: entry.relativePath,
    absolutePath: entry.absolutePath,
    name: entry.name,
    dirty: false,
  };
  store.openTab(tab);

  // Already cached? Just activate.
  if (Object.prototype.hasOwnProperty.call(store.contents, entry.relativePath)) {
    store.setActive(entry.relativePath);
    return;
  }

  store.setLoading(entry.relativePath, true);
  void (async () => {
    try {
      const text = await filesystem.readFileText(entry.absolutePath);
      useEditorStore.getState().setContent(entry.relativePath, text);
    } catch (err) {
      const message = getErrorMessage(err);
      useEditorStore.getState().setError(entry.relativePath, message);
    } finally {
      useEditorStore.getState().setLoading(entry.relativePath, false);
    }
  })();
}
