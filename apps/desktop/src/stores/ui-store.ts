import { create } from 'zustand';

/**
 * UI-only state: panel sizes, settings sheet visibility. Persisted to
 * localStorage under a versioned key so a future schema change can
 * migrate / drop instead of crashing.
 */

const STORAGE_KEY = 'testing-ide.ui.v1';
const DEFAULT_PANEL_SIZES: PanelSizes = [18, 56, 26];

export type PanelSizes = [number, number, number];

export type UiState = {
  panelSizes: PanelSizes;
  settingsOpen: boolean;
  setPanelSizes: (sizes: PanelSizes) => void;
  setSettingsOpen: (open: boolean) => void;
};

function isPanelSizes(value: unknown): value is PanelSizes {
  return (
    Array.isArray(value) &&
    value.length === 3 &&
    value.every((s) => typeof s === 'number' && Number.isFinite(s))
  );
}

function loadInitial(): Pick<UiState, 'panelSizes' | 'settingsOpen'> {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) {
      return { panelSizes: DEFAULT_PANEL_SIZES, settingsOpen: false };
    }
    const parsed: unknown = JSON.parse(raw);
    const sizes =
      typeof parsed === 'object' && parsed !== null && 'panelSizes' in parsed
        ? parsed.panelSizes
        : null;
    return {
      panelSizes: isPanelSizes(sizes) ? sizes : DEFAULT_PANEL_SIZES,
      settingsOpen: false,
    };
  } catch {
    return { panelSizes: DEFAULT_PANEL_SIZES, settingsOpen: false };
  }
}

function persist(state: Pick<UiState, 'panelSizes'>): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ panelSizes: state.panelSizes }));
  } catch {
    // localStorage unavailable — silently no-op so the app remains usable.
  }
}

const store = create<UiState>()((set, get) => {
  const initial = loadInitial();
  return {
    ...initial,
    setPanelSizes: (panelSizes) => {
      set({ panelSizes });
      persist({ panelSizes: get().panelSizes });
    },
    setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  };
});

const globalStore = globalThis as unknown as {
  useUiStore?: typeof store;
};

export const useUiStore = globalStore.useUiStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useUiStore = useUiStore;
}

