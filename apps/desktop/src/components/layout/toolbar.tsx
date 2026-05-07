import type { Project } from '@testing-ide/shared';
import { Clock, FolderOpen, Settings } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { analysis as analysisIpc, filesystem, IpcError, projects } from '@/lib/ipc';
import { useEditorStore } from '@/stores/editor-store';
import { useUiStore } from '@/stores/ui-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Top toolbar above the three-panel workspace. Hosts the "Open folder"
 * action (native dialog), the recent-projects popover (re-opens a row
 * persisted by `create_project`), and the Settings sheet trigger.
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

  const loadProject = useCallback(
    (target: Project, options: { skipAnalysisIfReady: boolean }) => {
      setTreeError(null);
      setTreeLoading(true);
      void (async () => {
        try {
          // Reset the editor first so stale tabs from a previous project
          // don't survive into the new one.
          useEditorStore.getState().reset();
          setProject(target);
          const entries = await filesystem.readDirectoryEntries(target.rootPath, '');
          setTree(entries);
        } catch (err) {
          setTreeError(err instanceof IpcError ? err.message : String(err));
        } finally {
          setTreeLoading(false);
        }

        // Skip analysis when the project is already `ready` and the
        // caller wants to short-circuit (re-opening a previously
        // analysed folder shouldn't re-run the whole pipeline).
        const alreadyReady =
          options.skipAnalysisIfReady && target.status === 'ready' && target.fileCount > 0;
        if (alreadyReady) return;

        setAnalysis({ status: 'pending' });
        try {
          const outcome = await analysisIpc.analyzeProject(target.id);
          try {
            const refreshed = await projects.getProject(target.id);
            updateProject(refreshed);
          } catch {
            // Non-fatal: analysis succeeded; stale project values are cosmetic.
          }
          setAnalysis({ status: 'ready', outcome });
        } catch (err) {
          setAnalysis({
            status: 'error',
            message: err instanceof IpcError ? err.message : String(err),
          });
        }
      })();
    },
    [setAnalysis, setProject, setTree, setTreeError, setTreeLoading, updateProject],
  );

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
      let created: Project;
      try {
        const name = deriveProjectName(path);
        created = await projects.createProject(name, path);
      } catch (err) {
        setTreeError(err instanceof IpcError ? err.message : String(err));
        return;
      }
      // Fresh-create flow always runs analysis even if backend stamped
      // a status (defensive — newly-discovered files always need to
      // hit the chunker).
      loadProject(created, { skipAnalysisIfReady: false });
    })();
  }, [loadProject, setTreeError]);

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
        <RecentProjectsButton
          activeId={project?.id ?? null}
          onSelect={(p) => loadProject(p, { skipAnalysisIfReady: true })}
        />
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
 * Recent-projects popover trigger. Lazy-loads the list on first open
 * so the toolbar mount doesn't burn an IPC round-trip when the user
 * never clicks it.
 */
function RecentProjectsButton({
  activeId,
  onSelect,
}: {
  activeId: string | null;
  onSelect: (project: Project) => void;
}) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<Project[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Close on outside click — the popover is non-modal so the rest of
  // the toolbar stays clickable.
  useEffect(() => {
    if (!open) return;
    const handler = (event: MouseEvent) => {
      const node = containerRef.current;
      if (node !== null && event.target instanceof Node && !node.contains(event.target)) {
        setOpen(false);
      }
    };
    window.addEventListener('mousedown', handler);
    return () => window.removeEventListener('mousedown', handler);
  }, [open]);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const next = await projects.listProjects();
        setList(next);
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const handleToggle = () => {
    setOpen((prev) => {
      const next = !prev;
      if (next && list === null) refresh();
      return next;
    });
  };

  return (
    <div ref={containerRef} className="relative">
      <Button
        type="button"
        size="sm"
        variant="ghost"
        onClick={handleToggle}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        <Clock className="size-4" />
        Recent
      </Button>
      {open ? (
        <div className="bg-card absolute right-0 top-full z-50 mt-1 w-72 overflow-hidden rounded-md border border-border shadow-lg">
          <div className="flex items-center justify-between border-b border-border px-3 py-1.5">
            <p className="text-muted-foreground text-[10px] font-semibold uppercase tracking-wider">
              Recent projects
            </p>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={refresh}
              className="h-6 px-1.5 text-[10px]"
            >
              Refresh
            </Button>
          </div>
          <div className="max-h-72 overflow-y-auto">
            {loading ? (
              <p className="text-muted-foreground p-3 text-xs">Loading…</p>
            ) : error !== null ? (
              <p className="text-destructive p-3 text-xs" role="alert">
                {error}
              </p>
            ) : list === null || list.length === 0 ? (
              <p className="text-muted-foreground p-3 text-xs">
                None yet. Open a folder to start.
              </p>
            ) : (
              <ul role="listbox">
                {list.map((p) => (
                  <li key={p.id}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={activeId === p.id}
                      onClick={() => {
                        onSelect(p);
                        setOpen(false);
                      }}
                      className={`hover:bg-muted/50 flex w-full flex-col items-start gap-0.5 px-3 py-2 text-left text-xs transition-colors ${
                        activeId === p.id ? 'bg-muted/30' : ''
                      }`}
                    >
                      <span className="truncate font-medium">{p.name}</span>
                      <span
                        className="text-muted-foreground truncate text-[10px]"
                        title={p.rootPath}
                      >
                        {p.rootPath}
                      </span>
                      <span className="text-muted-foreground text-[10px]">
                        {p.fileCount} files · {p.status}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      ) : null}
    </div>
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
