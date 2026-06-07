import type { IndexStatus } from '@testing-ide/shared';
import { create } from 'zustand';

import { embeddings } from '@/lib/ipc';

/**
 * Stale-index status for the open project
 * (plan/EMBEDDING_PROVIDER_SELECT.md §7.2).
 *
 * Centralised so every refresh trigger converges on one fetch path:
 * the banner refreshes on project open and analyze completion, and the
 * Settings sheet refreshes after an embedding-config save — without
 * the two components knowing about each other.
 */
type EmbeddingState = {
  indexStatus: IndexStatus | null;
  /** Refresh the status for one project. Errors clear the banner —
   *  stale-index info is advisory, never worth blocking the UI. */
  refreshIndexStatus: (projectId: string) => Promise<void>;
  /** Drop the banner state (project closed / deleted). */
  clearIndexStatus: () => void;
};

export const useEmbeddingStore = create<EmbeddingState>()((set) => ({
  indexStatus: null,
  refreshIndexStatus: async (projectId: string) => {
    try {
      const status = await embeddings.getIndexStatus(projectId);
      set({ indexStatus: status });
    } catch {
      set({ indexStatus: null });
    }
  },
  clearIndexStatus: () => {
    set({ indexStatus: null });
  },
}));
