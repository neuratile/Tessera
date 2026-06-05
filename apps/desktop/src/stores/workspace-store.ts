import type { AnalysisOutcome, Project } from '@testing-ide/shared';
import { create } from 'zustand';

import { filesystem, getErrorMessage } from '@/lib/ipc';

console.log("DEBUG: Evaluating workspace-store.ts module");

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
  isLoaded?: boolean;
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
  /** Re-reads the loaded directory structure from disk recursively. */
  refreshTree: () => Promise<void>;
  reset: () => void;
};

function replaceChildren(entries: FsEntry[], target: string, children: FsEntry[]): FsEntry[] {
  return entries.map((entry) => {
    if (entry.relativePath === target) {
      return { ...entry, children, isLoaded: true };
    }
    if (entry.children) {
      return { ...entry, children: replaceChildren(entry.children, target, children) };
    }
    return entry;
  });
}

async function refreshTreeHelper(
  oldEntries: FsEntry[],
  currentPath: string,
  relativePrefix: string,
): Promise<FsEntry[]> {
  let newEntries: FsEntry[];
  try {
    newEntries = await filesystem.readDirectoryEntries(currentPath, relativePrefix);
  } catch (err) {
    console.warn(`Failed to refresh directory ${currentPath}:`, err);
    return [];
  }

  const oldMap = new Map<string, FsEntry>();
  for (const entry of oldEntries) {
    oldMap.set(entry.relativePath, entry);
  }

  return Promise.all(
    newEntries.map(async (entry) => {
      if (entry.kind === 'directory') {
        const oldEntry = oldMap.get(entry.relativePath);
        if (oldEntry && oldEntry.isLoaded === true) {
          const refreshedChildren = await refreshTreeHelper(
            oldEntry.children ?? [],
            entry.absolutePath,
            entry.relativePath,
          );
          return { ...entry, children: refreshedChildren, isLoaded: true };
        }
      }
      return entry;
    })
  );
}

const store = create<WorkspaceState>()((set) => ({
  project: null,
  tree: [],
  loadingTree: false,
  treeError: null,
  selectedPath: null,
  analysis: { status: 'idle' },

  setProject: (project) => {
    console.log("DEBUG: setProject called with:", project);
    set({
      project,
      tree: [],
      treeError: null,
      selectedPath: null,
      analysis: { status: 'idle' },
    });
  },
  updateProject: (project) => set({ project }),
  setTree: (tree) => set({ tree, treeError: null }),
  setTreeLoading: (loadingTree) => set({ loadingTree }),
  setTreeError: (treeError) => set({ treeError, loadingTree: false }),
  setSelectedPath: (selectedPath) => set({ selectedPath }),
  setAnalysis: (analysis) => set({ analysis }),
  setChildren: (relativePath, children) =>
    set((state) => ({ tree: replaceChildren(state.tree, relativePath, children) })),
  refreshTree: async () => {
    const { project, tree } = useWorkspaceStore.getState();
    if (project === null) return;
    try {
      const refreshed = await refreshTreeHelper(tree, project.rootPath, '');
      set({ tree: refreshed, treeError: null });
    } catch (err) {
      set({ treeError: getErrorMessage(err) });
    }
  },
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

const globalStore = globalThis as unknown as {
  useWorkspaceStore?: typeof store;
};

export const useWorkspaceStore = globalStore.useWorkspaceStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useWorkspaceStore = useWorkspaceStore;
}

