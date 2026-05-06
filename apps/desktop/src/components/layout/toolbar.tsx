import { FolderOpen, Settings } from 'lucide-react';
import { useCallback } from 'react';

import { Button } from '@/components/ui/button';
import { analysis as analysisIpc, filesystem, IpcError, projects } from '@/lib/ipc';
import { useEditorStore } from '@/stores/editor-store';
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
  const setAnalysis = useWorkspaceStore((s) => s.setAnalysis);
  const updateProject = useWorkspaceStore((s) => s.updateProject);

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
      let createdId: string | null = null;
      try {
        const name = deriveProjectName(path);
        const created = await projects.createProject(name, path);
        createdId = created.id;
        // Reset the editor first so stale tabs from a previous project
        // don't survive into the new one.
        useEditorStore.getState().reset();
        setProject(created);
        const entries = await filesystem.readDirectoryEntries(path, '');
        setTree(entries);
      } catch (err) {
        setTreeError(err instanceof IpcError ? err.message : String(err));
      } finally {
        setTreeLoading(false);
      }

      // Kick off analysis after the explorer renders so the user is
      // not staring at an empty tree while AST + embeddings run.
      // Analysis populates `code_chunks` — without it, RAG retrieval
      // would return zero hits and `generate_artifact` would emit
      // empty / nonsense output.
      if (createdId !== null) {
        setAnalysis({ status: 'pending' });
        try {
          const outcome = await analysisIpc.analyzeProject(createdId);
          // Patch project (status / fileCount / totalSizeBytes)
          // without nuking the tree we just loaded. `updateProject`
          // is the merge-only setter for this case.
          try {
            const refreshed = await projects.getProject(createdId);
            updateProject(refreshed);
          } catch {
            // Non-fatal: analysis succeeded; stale project values are
            // cosmetic.
          }
          setAnalysis({ status: 'ready', outcome });
        } catch (err) {
          setAnalysis({
            status: 'error',
            message: err instanceof IpcError ? err.message : String(err),
          });
        }
      }
    })();
  }, [setAnalysis, setProject, setTree, setTreeError, setTreeLoading, updateProject]);

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
