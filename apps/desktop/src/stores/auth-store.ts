import { create } from 'zustand';

/**
 * In-memory session tokens (cleared on reload). Pair with `register` /
 * `login` / `refresh_token` IPC — no disk persistence in Phase 5.
 */
type AuthState = {
  accessToken: string | null;
  refreshToken: string | null;
  setTokens: (accessToken: string, refreshToken: string) => void;
  clear: () => void;
};

const store = create<AuthState>((set) => ({
  accessToken: null,
  refreshToken: null,
  setTokens: (accessToken, refreshToken) => set({ accessToken, refreshToken }),
  clear: () => set({ accessToken: null, refreshToken: null }),
}));

const globalStore = globalThis as unknown as {
  useAuthStore?: typeof store;
};

export const useAuthStore = globalStore.useAuthStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useAuthStore = useAuthStore;
}

