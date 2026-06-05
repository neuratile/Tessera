import { create } from 'zustand';
import { persist } from 'zustand/middleware';

type AuthState = {
  accessToken: string | null;
  refreshToken: string | null;
  setTokens: (accessToken: string, refreshToken: string) => void;
  clear: () => void;
};

const store = create<AuthState>()(
  persist(
    (set) => ({
      accessToken: null,
      refreshToken: null,
      setTokens: (accessToken, refreshToken) => set({ accessToken, refreshToken }),
      clear: () => set({ accessToken: null, refreshToken: null }),
    }),
    {
      name: 'tessera-auth-storage',
    }
  )
);

const globalStore = globalThis as unknown as {
  useAuthStore?: typeof store;
};

export const useAuthStore = globalStore.useAuthStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useAuthStore = useAuthStore;
}
