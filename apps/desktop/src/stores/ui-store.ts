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

function loadInitial(): Pick<UiState, 'panelSizes' | 'settingsOpen'> {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) {
      return { panelSizes: DEFAULT_PANEL_SIZES, settingsOpen: false };
    }
    const parsed = JSON.parse(raw) as { panelSizes?: unknown; settingsOpen?: unknown };
    const sizes = Array.isArray(parsed.panelSizes) ? parsed.panelSizes : null;
    const valid =
      sizes !== null &&
      sizes.length === 3 &&
      sizes.every((s) => typeof s === 'number' && Number.isFinite(s));
    return {
      panelSizes: valid ? (sizes as PanelSizes) : DEFAULT_PANEL_SIZES,
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

export const useUiStore = create<UiState>()((set, get) => {
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
