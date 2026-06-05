import type {
  ArtifactSummary,
  GenerationArtifactType,
  ProviderConfigView,
} from '@testing-ide/shared';
import { create } from 'zustand';

/**
 * AI panel state — review queue + generation status.
 *
 * Generation calls the existing Phase 5 `generate_artifact` IPC command.
 * Streaming token updates are deferred until the backend exposes a
 * Tauri-event sink; for now generation is request/response with a
 * `pending` flag while the await is in flight.
 */

export type GenerationPending = {
  status: 'pending';
  artifactType: GenerationArtifactType;
  /** Streaming buffer accumulated from `tool_args` / `text` events.
   *  Used by the AI panel to render a live preview while the await
   *  for `generate_artifact` is in flight. */
  partial: string;
};

export type GenerationStatus =
  | { status: 'idle' }
  | GenerationPending
  | { status: 'error'; message: string };

export type AiState = {
  generation: GenerationStatus;
  artifacts: ArtifactSummary[];
  loadingArtifacts: boolean;
  artifactsError: string | null;
  /** All configured providers, populated on first AI-panel mount and
   *  on every provider mutation (save / delete / set-active). Read by
   *  the status-bar provider switcher and AI panel itself. */
  providers: ProviderConfigView[];
  /** Active provider chosen by the user. `null` means "first active row
   *  from `list_provider_configs`". */
  activeProvider: ProviderConfigView | null;

  setGeneration: (status: GenerationStatus) => void;
  /** Append a chunk to the streaming buffer. No-op when generation is
   *  not currently `pending` so a stale event from a cancelled run
   *  cannot mutate the state. */
  appendPartial: (delta: string) => void;
  setArtifacts: (artifacts: ArtifactSummary[]) => void;
  upsertArtifact: (artifact: ArtifactSummary) => void;
  setLoadingArtifacts: (loading: boolean) => void;
  setArtifactsError: (error: string | null) => void;
  setProviders: (providers: ProviderConfigView[]) => void;
  setActiveProvider: (provider: ProviderConfigView | null) => void;
  reset: () => void;
};

const store = create<AiState>()((set) => ({
  generation: { status: 'idle' },
  artifacts: [],
  loadingArtifacts: false,
  artifactsError: null,
  providers: [],
  activeProvider: null,

  setGeneration: (generation) => set({ generation }),
  appendPartial: (delta) =>
    set((state) =>
      state.generation.status === 'pending'
        ? {
            generation: {
              ...state.generation,
              partial: state.generation.partial + delta,
            },
          }
        : state,
    ),
  setArtifacts: (artifacts) => set({ artifacts, artifactsError: null }),
  upsertArtifact: (artifact) =>
    set((state) => {
      const existing = state.artifacts.findIndex((a) => a.id === artifact.id);
      if (existing === -1) {
        return { artifacts: [artifact, ...state.artifacts] };
      }
      const next = [...state.artifacts];
      next[existing] = artifact;
      return { artifacts: next };
    }),
  setLoadingArtifacts: (loadingArtifacts) => set({ loadingArtifacts }),
  setArtifactsError: (artifactsError) => set({ artifactsError, loadingArtifacts: false }),
  setProviders: (providers) => set({ providers }),
  setActiveProvider: (activeProvider) => set({ activeProvider }),
  reset: () =>
    set({
      generation: { status: 'idle' },
      artifacts: [],
      loadingArtifacts: false,
      artifactsError: null,
    }),
}));

const globalStore = globalThis as unknown as {
  useAiStore?: typeof store;
};

export const useAiStore = globalStore.useAiStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useAiStore = useAiStore;
}

