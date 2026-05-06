import { create } from 'zustand'
import { persist } from 'zustand/middleware'

type Theme = 'dark' | 'light'

interface UiState {
  theme: Theme
  isSettingsOpen: boolean
  panelSizes: number[]
  setTheme: (theme: Theme) => void
  setIsSettingsOpen: (isOpen: boolean) => void
  setPanelSizes: (sizes: number[]) => void
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      theme: 'dark',
      isSettingsOpen: false,
      panelSizes: [20, 55, 25],
      setTheme: (theme) => set({ theme }),
      setIsSettingsOpen: (isOpen) => set({ isSettingsOpen: isOpen }),
      setPanelSizes: (sizes) => set({ panelSizes: sizes }),
    }),
    {
      name: 'ui-storage',
      partialize: (state) => ({ theme: state.theme, panelSizes: state.panelSizes }),
    }
  )
)
