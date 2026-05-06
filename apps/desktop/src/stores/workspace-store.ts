import type { AnalysisOutcome, Project } from '@testing-ide/shared';
import { create } from 'zustand';

/**
 * Workspace store: tracks the currently-open project and the in-memory
 * file tree built from the Tauri filesystem walk. The Phase 6 backend
 * persists projects (`create_project`); this store mirrors only the
 * subset the UI needs at runtime.
 */

export type FsEntry = {
  /** Stable id for `react-arborist` — relative path is unique within a project. */
  id: string;
  /** Display name (basename). */
  name: string;
  /** Path relative to the project root, forward-slash separated. */
  relativePath: string;
  /** Absolute path on disk — used by file-content reads. */
  absolutePath: string;
  kind: 'file' | 'directory';
  children?: FsEntry[];
};

export type AnalysisState =
  | { status: 'idle' }
  | { status: 'pending' }
  | { status: 'ready'; outcome: AnalysisOutcome }
  | { status: 'error'; message: string };

export type WorkspaceState = {
  project: Project | null;
  /** Top-level entries under `project.rootPath`. Lazy-loaded per directory. */
  tree: FsEntry[];
  loadingTree: boolean;
  treeError: string | null;
  selectedPath: string | null;
  analysis: AnalysisState;

  setProject: (project: Project | null) => void;
  /** Update only the `project` field without resetting the tree /
   *  analysis state. Used after `analyze_project` completes so the
   *  refreshed `fileCount` / `status` propagate without nuking the
   *  loaded directory walk. */
  updateProject: (project: Project) => void;
  setTree: (tree: FsEntry[]) => void;
  setTreeLoading: (loading: boolean) => void;
  setTreeError: (error: string | null) => void;
  setSelectedPath: (path: string | null) => void;
  setAnalysis: (state: AnalysisState) => void;
  /** Replace the children of the entry at `relativePath`. Used after a
   *  lazy directory expand. */
  setChildren: (relativePath: string, children: FsEntry[]) => void;
  reset: () => void;
};

function replaceChildren(entries: FsEntry[], target: string, children: FsEntry[]): FsEntry[] {
  return entries.map((entry) => {
    if (entry.relativePath === target) {
      return { ...entry, children };
    }
    if (entry.children) {
      return { ...entry, children: replaceChildren(entry.children, target, children) };
    }
    return entry;
  });
}

export const useWorkspaceStore = create<WorkspaceState>()((set) => ({
  project: null,
  tree: [],
  loadingTree: false,
  treeError: null,
  selectedPath: null,
  analysis: { status: 'idle' },

  setProject: (project) =>
    set({
      project,
      tree: [],
      treeError: null,
      selectedPath: null,
      analysis: { status: 'idle' },
    }),
  updateProject: (project) => set({ project }),
  setTree: (tree) => set({ tree, treeError: null }),
  setTreeLoading: (loadingTree) => set({ loadingTree }),
  setTreeError: (treeError) => set({ treeError, loadingTree: false }),
  setSelectedPath: (selectedPath) => set({ selectedPath }),
  setAnalysis: (analysis) => set({ analysis }),
  setChildren: (relativePath, children) =>
    set((state) => ({ tree: replaceChildren(state.tree, relativePath, children) })),
  reset: () =>
    set({
      project: null,
      tree: [],
      loadingTree: false,
      treeError: null,
      selectedPath: null,
      analysis: { status: 'idle' },
    }),
}));
