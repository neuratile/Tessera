import { FolderOpen, Settings } from 'lucide-react';
import { useCallback } from 'react';

import { Button } from '@/components/ui/button';
import { filesystem, IpcError, projects } from '@/lib/ipc';
import { useUiStore } from '@/stores/ui-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Top toolbar above the three-panel workspace. Hosts the "Open folder"
 * action (Tauri native dialog), and the Settings sheet trigger. Auth +
 * profile controls land in a later phase.
 */
export function Toolbar() {
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const project = useWorkspaceStore((s) => s.project);
  const setProject = useWorkspaceStore((s) => s.setProject);
  const setTree = useWorkspaceStore((s) => s.setTree);
  const setTreeLoading = useWorkspaceStore((s) => s.setTreeLoading);
  const setTreeError = useWorkspaceStore((s) => s.setTreeError);

  const handleOpenFolder = useCallback(() => {
    setTreeError(null);
    void (async () => {
      let path: string | null;
      try {
        path = await filesystem.pickFolder();
      } catch (err) {
        setTreeError(err instanceof IpcError ? err.message : String(err));
        return;
      }
      if (path === null) return; // user cancelled
      setTreeLoading(true);
      try {
        const name = deriveProjectName(path);
        const created = await projects.createProject(name, path);
        setProject(created);
        const entries = await filesystem.readDirectoryEntries(path, '');
        setTree(entries);
      } catch (err) {
        setTreeError(err instanceof IpcError ? err.message : String(err));
      } finally {
        setTreeLoading(false);
      }
    })();
  }, [setProject, setTree, setTreeError, setTreeLoading]);

  return (
    <header className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-card px-3">
      <div className="flex items-center gap-2">
        <span className="text-sm font-semibold tracking-tight">Testing IDE</span>
        {project ? (
          <span className="text-muted-foreground truncate text-xs" title={project.rootPath}>
            · {project.name}
          </span>
        ) : null}
      </div>
      <div className="flex items-center gap-1">
        <Button type="button" size="sm" variant="ghost" onClick={handleOpenFolder}>
          <FolderOpen className="size-4" />
          Open folder
        </Button>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          aria-label="Settings"
          onClick={() => setSettingsOpen(true)}
        >
          <Settings className="size-4" />
        </Button>
      </div>
    </header>
  );
}

/**
 * Derive a sensible default project name from the chosen folder path.
 * Backend validates the name is non-empty after trim, so we fall back
 * to a generic label rather than letting `create_project` reject.
 */
function deriveProjectName(absolutePath: string): string {
  const parts = absolutePath.split(/[\\/]/u).filter((s) => s.length > 0);
  const last = parts[parts.length - 1];
  if (last !== undefined && last.length > 0) return last;
  return 'Untitled project';
}
