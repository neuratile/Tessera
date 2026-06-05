import { create } from 'zustand';

/**
 * Editor state — open tabs + per-tab content cache.
 *
 * Phase 10 ships read-only viewing. `dirty` is wired but no save path
 * yet — Phase 11+ adds persistence. Content is cached by relative path
 * so re-opening a tab does not re-read the disk.
 */

export type EditorTab = {
  /** Stable id — relative path within the project. */
  id: string;
  relativePath: string;
  absolutePath: string;
  /** File basename used in the tab strip. */
  name: string;
  /** True when the in-memory buffer diverges from disk. Phase 10 sets
   *  this on every keystroke; saving lands in a later phase. */
  dirty: boolean;
};

export type EditorState = {
  /** Open tabs in display order. */
  tabs: EditorTab[];
  activeId: string | null;
  /** path → content cache. Populated lazily from disk reads. */
  contents: Record<string, string>;
  /** path → load error. Lets the editor pane render a typed message
   *  instead of an empty buffer when a read fails. */
  errors: Record<string, string>;
  /** path → "loading from disk" flag. */
  loading: Record<string, boolean>;

  openTab: (tab: EditorTab) => void;
  closeTab: (id: string) => void;
  setActive: (id: string) => void;
  setContent: (relativePath: string, content: string) => void;
  setError: (relativePath: string, error: string | null) => void;
  setLoading: (relativePath: string, loading: boolean) => void;
  /** Mark buffer as edited. Phase 10 only flips `dirty`; no persistence. */
  markDirty: (relativePath: string, dirty: boolean) => void;
  /** Drop everything — called when the project is reset / replaced. */
  reset: () => void;
};

const store = create<EditorState>()((set) => ({
  tabs: [],
  activeId: null,
  contents: {},
  errors: {},
  loading: {},

  openTab: (tab) =>
    set((state) => {
      const existing = state.tabs.find((t) => t.id === tab.id);
      if (existing !== undefined) {
        return { activeId: existing.id };
      }
      return {
        tabs: [...state.tabs, tab],
        activeId: tab.id,
      };
    }),

  closeTab: (id) =>
    set((state) => {
      const idx = state.tabs.findIndex((t) => t.id === id);
      if (idx === -1) return state;
      const next = state.tabs.filter((t) => t.id !== id);
      let nextActive = state.activeId;
      if (state.activeId === id) {
        // Prefer the tab to the right of the closed one so close-all
        // behaviour is intuitive; fall back to the new last tab.
        const replacement = next[idx] ?? next[next.length - 1];
        nextActive = replacement?.id ?? null;
      }
      const { [id]: _droppedContent, ...contents } = state.contents;
      const { [id]: _droppedError, ...errors } = state.errors;
      const { [id]: _droppedLoading, ...loading } = state.loading;
      void _droppedContent;
      void _droppedError;
      void _droppedLoading;
      return { tabs: next, activeId: nextActive, contents, errors, loading };
    }),

  setActive: (id) =>
    set((state) => (state.tabs.some((t) => t.id === id) ? { activeId: id } : state)),

  setContent: (relativePath, content) =>
    set((state) => ({
      contents: { ...state.contents, [relativePath]: content },
      errors: stripKey(state.errors, relativePath),
    })),

  setError: (relativePath, error) =>
    set((state) => {
      if (error === null) {
        return { errors: stripKey(state.errors, relativePath) };
      }
      return { errors: { ...state.errors, [relativePath]: error } };
    }),

  setLoading: (relativePath, loading) =>
    set((state) => ({
      loading: loading
        ? { ...state.loading, [relativePath]: true }
        : stripKey(state.loading, relativePath),
    })),

  markDirty: (relativePath, dirty) =>
    set((state) => ({
      tabs: state.tabs.map((t) => (t.relativePath === relativePath ? { ...t, dirty } : t)),
    })),

  reset: () =>
    set({
      tabs: [],
      activeId: null,
      contents: {},
      errors: {},
      loading: {},
    }),
}));

const globalStore = globalThis as unknown as {
  useEditorStore?: typeof store;
};

export const useEditorStore = globalStore.useEditorStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useEditorStore = useEditorStore;
}


function stripKey<T>(obj: Record<string, T>, key: string): Record<string, T> {
  if (!(key in obj)) return obj;
  const { [key]: _dropped, ...rest } = obj;
  void _dropped;
  return rest;
}
