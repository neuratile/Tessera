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
  /**
   * Opt-in for local sandbox test execution. Off by default — the core
   * "no code execution on the default path" guarantee (plan §3). Persisted
   * so the choice survives restarts. The backend independently rejects runs
   * unless the request also carries `optInConfirmed: true`.
   */
  sandboxOptIn: boolean;
  setPanelSizes: (sizes: PanelSizes) => void;
  setSettingsOpen: (open: boolean) => void;
  setSandboxOptIn: (enabled: boolean) => void;
};

function isPanelSizes(value: unknown): value is PanelSizes {
  return (
    Array.isArray(value) &&
    value.length === 3 &&
    value.every((s) => typeof s === 'number' && Number.isFinite(s))
  );
}

function loadInitial(): Pick<UiState, 'panelSizes' | 'settingsOpen' | 'sandboxOptIn'> {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) {
      return { panelSizes: DEFAULT_PANEL_SIZES, settingsOpen: false, sandboxOptIn: false };
    }
    const parsed: unknown = JSON.parse(raw);
    const obj = typeof parsed === 'object' && parsed !== null ? parsed : {};
    const sizes = 'panelSizes' in obj ? obj.panelSizes : null;
    const optIn = 'sandboxOptIn' in obj ? obj.sandboxOptIn : null;
    return {
      panelSizes: isPanelSizes(sizes) ? sizes : DEFAULT_PANEL_SIZES,
      settingsOpen: false,
      sandboxOptIn: optIn === true,
    };
  } catch {
    return { panelSizes: DEFAULT_PANEL_SIZES, settingsOpen: false, sandboxOptIn: false };
  }
}

function persist(state: Pick<UiState, 'panelSizes' | 'sandboxOptIn'>): void {
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ panelSizes: state.panelSizes, sandboxOptIn: state.sandboxOptIn }),
    );
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
      persist({ panelSizes: get().panelSizes, sandboxOptIn: get().sandboxOptIn });
    },
    setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
    setSandboxOptIn: (sandboxOptIn) => {
      set({ sandboxOptIn });
      persist({ panelSizes: get().panelSizes, sandboxOptIn: get().sandboxOptIn });
    },
  };
});

const globalStore = globalThis as unknown as {
  useUiStore?: typeof store;
};

export const useUiStore = globalStore.useUiStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useUiStore = useUiStore;
}

