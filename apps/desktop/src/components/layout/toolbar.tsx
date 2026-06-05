import type { Project } from '@testing-ide/shared';
import { Clock, FolderOpen, Loader2, Settings, Trash } from 'lucide-react';
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react';

import { Button } from '@/components/ui/button';
import { COMMAND, useCommand } from '@/lib/command-bus';
import { analysis as analysisIpc, filesystem, getErrorMessage, projects } from '@/lib/ipc';
import { useEditorStore } from '@/stores/editor-store';
import { toast } from '@/stores/toast-store';
import { useUiStore } from '@/stores/ui-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Top toolbar above the three-panel workspace. Hosts the "Open folder"
 * action (native dialog), the recent-projects popover (re-opens a row
 * persisted by `create_project`), a manual Analyze button, and the
 * Settings sheet trigger.
 */
export function Toolbar() {
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const project = useWorkspaceStore((s) => s.project);
  const setProject = useWorkspaceStore((s) => s.setProject);
  const updateProject = useWorkspaceStore((s) => s.updateProject);
  const setTree = useWorkspaceStore((s) => s.setTree);
  const setTreeLoading = useWorkspaceStore((s) => s.setTreeLoading);
  const setTreeError = useWorkspaceStore((s) => s.setTreeError);
  const analysisState = useWorkspaceStore((s) => s.analysis);
  const setAnalysis = useWorkspaceStore((s) => s.setAnalysis);

  const runAnalysis = useCallback(
    (target: Project) => {
      setAnalysis({ status: 'pending' });
      void (async () => {
        try {
          const outcome = await analysisIpc.analyzeProject(target.id);
          try {
            const refreshed = await projects.getProject(target.id);
            updateProject(refreshed);
          } catch {
            // Non-fatal: stale project values are cosmetic.
          }
          setAnalysis({ status: 'ready', outcome });
        } catch (err) {
          setAnalysis({
            status: 'error',
            message: getErrorMessage(err),
          });
        }
      })();
    },
    [setAnalysis, updateProject],
  );

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
          setTreeError(getErrorMessage(err));
        } finally {
          setTreeLoading(false);
        }

        // Skip analysis when the project is already `ready` and the
        // caller wants to short-circuit (re-opening a previously
        // analysed folder shouldn't re-run the whole pipeline).
        const alreadyReady =
          options.skipAnalysisIfReady && target.status === 'ready' && target.fileCount > 0;
        if (alreadyReady) {
          try {
            const outcome = await analysisIpc.getAnalysisOutcome(target.id);
            if (outcome !== null) {
              setAnalysis({ status: 'ready', outcome });
            }
          } catch (err) {
            console.warn('Failed to load existing analysis outcome:', err);
          }
          return;
        }

        runAnalysis(target);
      })();
    },
    [runAnalysis, setProject, setTree, setTreeError, setTreeLoading, setAnalysis],
  );

  const handleOpenFolder = useCallback(() => {
    setTreeError(null);
    setAnalysis({ status: 'idle' });
    void (async () => {
      let path: string | null;
      try {
        path = await filesystem.pickFolder();
      } catch (err) {
        setTreeError(getErrorMessage(err));
        return;
      }
      if (path === null) return; // user cancelled
      let created: Project;
      try {
        const name = deriveProjectName(path);
        created = await projects.createProject(name, path);
      } catch (err) {
        setTreeError(getErrorMessage(err));
        return;
      }
      // Fresh-create flow always runs analysis even if backend stamped
      // a status (defensive — newly-discovered files always need to
      // hit the chunker).
      loadProject(created, { skipAnalysisIfReady: false });
    })();
  }, [loadProject, setAnalysis, setTreeError]);

  const handleAnalyze = useCallback(() => {
    if (project === null) return;
    runAnalysis(project);
  }, [project, runAnalysis]);

  // Command-bus subscriptions — fire the same handlers when the user
  // hits the native menu items or keyboard shortcuts (`Cmd/Ctrl+O`,
  // `Cmd/Ctrl+Shift+A`). No state is bypassed — handleOpenFolder /
  // handleAnalyze are the same callbacks the buttons invoke.
  useCommand(COMMAND.FileOpenFolder, handleOpenFolder);
  useCommand(COMMAND.AiAnalyze, handleAnalyze);

  const isAnalyzing = analysisState.status === 'pending';

  // Push analysis terminal-state into the global toast stack so the
  // bottom-right viewport renders it consistently with every other
  // notification source. Re-fires on every transition out of
  // `pending`, including back-to-back analyses.
  const lastStatusRef = useRef(analysisState.status);
  useEffect(() => {
    const previous = lastStatusRef.current;
    lastStatusRef.current = analysisState.status;
    if (previous !== 'pending') return;
    if (analysisState.status === 'ready') {
      const o = analysisState.outcome;
      toast.ok(
        `Indexed ${o.chunksEmbedded} chunks · ${o.filesParsed}/${o.filesDiscovered} files`,
        { title: 'Analyze complete' },
      );
    } else if (analysisState.status === 'error') {
      toast.err(analysisState.message, { title: 'Analyze failed' });
    }
  }, [analysisState]);

  console.log("DEBUG: Toolbar rendering, project is:", project);
  return (
    <header className="flex h-8 shrink-0 items-center justify-between border-b border-border bg-card px-3">
      <div className="flex min-w-0 items-center gap-2.5">
        <img
          src="/tessera-logo.png"
          alt="Tessera"
          className="size-6 rounded-sm shrink-0"
          draggable="false"
        />
        <span className="font-brand text-primary text-base">tessera</span>
        {project ? (
          <>
            <span className="text-border" aria-hidden="true">
              ·
            </span>
            <span
              className="text-muted-foreground truncate font-mono text-xs"
              title={project.rootPath}
            >
              {project.name}
            </span>
          </>
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
          size="sm"
          variant="ghost"
          onClick={handleAnalyze}
          disabled={project === null || isAnalyzing}
          aria-label="Analyze project"
          data-testid="analyze-project"
        >
          {isAnalyzing ? <Loader2 className="size-4 animate-spin" /> : null}
          {isAnalyzing ? 'Analyzing…' : 'Analyze'}
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
        setError(getErrorMessage(err));
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

  const handleDeleteProject = useCallback(
    async (id: string, name: string) => {
      try {
        const confirmed = await filesystem.confirm(
          `Are you sure you want to delete project "${name}"? This will remove it from the recent projects list, but your files on disk will not be touched.`,
          { title: 'Delete Project', kind: 'warning' },
        );
        if (!confirmed) return;

        await projects.deleteProject(id);
        setList((prev) => (prev ? prev.filter((p) => p.id !== id) : null));
        toast.ok('Project removed from recents');
        if (activeId === id) {
          useEditorStore.getState().reset();
          useWorkspaceStore.getState().reset();
        }
      } catch (err) {
        toast.err(getErrorMessage(err), { title: 'Failed to delete project' });
      }
    },
    [activeId],
  );

  // Keyboard nav for the listbox — arrow keys move the highlight,
  // Enter selects, Escape closes. Highlight is reset to 0 when the
  // list refreshes so an upstream delete cannot leave it pointing
  // past the end of the array.
  const [highlight, setHighlight] = useState(0);
  useEffect(() => {
    setHighlight(0);
  }, [list]);

  const select = useCallback(
    (project: Project) => {
      onSelect(project);
      setOpen(false);
    },
    [onSelect],
  );

  const onListKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      const items = list ?? [];
      if (event.key === 'Escape') {
        event.preventDefault();
        setOpen(false);
        return;
      }
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setHighlight((h) => Math.min(items.length - 1, h + 1));
        return;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setHighlight((h) => Math.max(0, h - 1));
        return;
      }
      if (event.key === 'Home') {
        event.preventDefault();
        setHighlight(0);
        return;
      }
      if (event.key === 'End') {
        event.preventDefault();
        setHighlight(Math.max(0, items.length - 1));
        return;
      }
      if (event.key === 'Enter') {
        const target = items[highlight];
        if (target !== undefined) {
          event.preventDefault();
          select(target);
        }
      }
    },
    [list, highlight, select],
  );

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
        <div
          className="bg-card absolute right-0 top-full z-50 mt-1 w-72 overflow-hidden rounded-md border border-border shadow-lg outline-none"
          tabIndex={-1}
          onKeyDown={onListKeyDown}
          // Focus the popover on first render so Arrow / Enter / Esc
          // are captured without the user clicking inside first.
          ref={(node) => {
            if (node !== null && open) node.focus();
          }}
        >
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
              <ul role="listbox" aria-label="Recent projects">
                {list.map((p, index) => {
                  const isHighlighted = index === highlight;
                  return (
                    <li key={p.id} className="relative group/row flex items-center justify-between">
                      <button
                        type="button"
                        role="option"
                        aria-selected={activeId === p.id}
                        onMouseMove={() => {
                          if (!isHighlighted) setHighlight(index);
                        }}
                        onClick={() => select(p)}
                        className={`flex-1 flex flex-col items-start gap-0.5 px-3 py-2 text-left text-xs transition-colors pr-10 ${
                          isHighlighted ? 'bg-primary/10 text-primary' : ''
                        } ${activeId === p.id && !isHighlighted ? 'bg-muted/30' : ''}`}
                      >
                        <span className="truncate font-medium">{p.name}</span>
                        <span
                          className="text-muted-foreground truncate text-[10px] max-w-[200px]"
                          title={p.rootPath}
                        >
                          {p.rootPath}
                        </span>
                        <span className="text-muted-foreground text-[10px]">
                          {p.fileCount} files · {p.status}
                        </span>
                      </button>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          void handleDeleteProject(p.id, p.name);
                        }}
                        className="absolute right-2 opacity-0 group-hover/row:opacity-100 focus:opacity-100 hover:text-destructive text-muted-foreground p-1 transition-opacity"
                        aria-label={`Delete ${p.name}`}
                      >
                        <Trash className="size-3.5" />
                      </button>
                    </li>
                  );
                })}
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
