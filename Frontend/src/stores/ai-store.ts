import { create } from 'zustand'
import { AppError } from '@/lib/error'

export type AiProvider = 'ollama' | 'openai' | 'anthropic' | 'openrouter'

export type GenerationState =
  | { status: 'idle' }
  | { status: 'streaming'; progress: number; currentFile: string }
  | { status: 'done'; generatedCount: number }
  | { status: 'error'; error: AppError }

export type ReviewItem = {
  id: string
  filePath: string
  generatedTest: string
  status: 'pending' | 'approved' | 'rejected' | 'regenerating'
  feedback: string
}

interface AiState {
  provider: AiProvider
  apiKey: string
  baseUrl: string
  selectedModel: string
  
  generationState: GenerationState
  reviewQueue: ReviewItem[]
  
  setProviderConfig: (config: Partial<{ provider: AiProvider; apiKey: string; baseUrl: string; selectedModel: string }>) => void
  setGenerationState: (state: GenerationState) => void
  setReviewQueue: (queue: ReviewItem[]) => void
  updateReviewItem: (id: string, updates: Partial<ReviewItem>) => void
  clearReviewQueue: () => void
}

export const useAiStore = create<AiState>()((set) => ({
  provider: 'ollama',
  apiKey: '',
  baseUrl: 'http://localhost:11434',
  selectedModel: '',
  
  generationState: { status: 'idle' },
  reviewQueue: [],
  
  setProviderConfig: (config) => set((state) => ({ ...state, ...config })),
  setGenerationState: (state) => set({ generationState: state }),
  setReviewQueue: (queue) => set({ reviewQueue: queue }),
  updateReviewItem: (id, updates) => set((state) => ({
    reviewQueue: state.reviewQueue.map(item => item.id === id ? { ...item, ...updates } : item)
  })),
  clearReviewQueue: () => set({ reviewQueue: [] }),
}))
