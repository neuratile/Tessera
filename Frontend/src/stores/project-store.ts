import { create } from 'zustand'
import { AppError } from '@/lib/error'

export type ProjectFile = {
  id: string
  name: string
  path: string
  type: 'file' | 'directory'
  language: string | null
  children?: ProjectFile[]
  content?: string
}

export type UploadState =
  | { status: 'idle' }
  | { status: 'uploading'; progress: number }
  | { status: 'ready'; projectName: string; files: ProjectFile[] }
  | { status: 'error'; error: AppError }

export type EditorTabItem = {
  id: string
  path: string
  name: string
  isUnsaved: boolean
}

interface ProjectState {
  uploadState: UploadState
  selectedFilePath: string | null
  openTabs: EditorTabItem[]
  activeTabId: string | null
  fileContents: Record<string, string> // path -> content
  
  setUploadState: (state: UploadState) => void
  setSelectedFilePath: (path: string | null) => void
  setFiles: (projectName: string, files: ProjectFile[]) => void
  resetProject: () => void
  openFile: (file: ProjectFile) => void
  closeTab: (tabId: string) => void
  setActiveTab: (tabId: string) => void
  updateFileContent: (path: string, content: string) => void
}

export const useProjectStore = create<ProjectState>()((set, get) => ({
  uploadState: { status: 'idle' },
  selectedFilePath: null,
  openTabs: [],
  activeTabId: null,
  fileContents: {},

  setUploadState: (state) => set({ uploadState: state }),
  
  setSelectedFilePath: (path) => set({ selectedFilePath: path }),

  setFiles: (projectName, files) => set({ 
    uploadState: { status: 'ready', projectName, files },
    selectedFilePath: null,
    openTabs: [],
    activeTabId: null,
    fileContents: {}
  }),

  resetProject: () => set({
    uploadState: { status: 'idle' },
    selectedFilePath: null,
    openTabs: [],
    activeTabId: null,
    fileContents: {}
  }),
  
  openFile: (file) => {
    if (file.type !== 'file') return
    const { openTabs, fileContents } = get()
    const existingTab = openTabs.find((t) => t.path === file.path)
    
    if (existingTab) {
      set({ activeTabId: existingTab.id, selectedFilePath: file.path })
      return
    }
    
    const newTab: EditorTabItem = {
      id: file.id,
      path: file.path,
      name: file.name,
      isUnsaved: false,
    }
    
    set({
      openTabs: [...openTabs, newTab],
      activeTabId: newTab.id,
      selectedFilePath: file.path,
      fileContents: {
        ...fileContents,
        [file.path]: file.content || '',
      }
    })
  },
  
  closeTab: (tabId) => {
    const { openTabs, activeTabId } = get()
    const updatedTabs = openTabs.filter(t => t.id !== tabId)
    let newActiveTabId = activeTabId
    
    if (activeTabId === tabId) {
      newActiveTabId = updatedTabs.length > 0 ? updatedTabs[updatedTabs.length - 1]!.id : null
    }
    
    set({ 
      openTabs: updatedTabs, 
      activeTabId: newActiveTabId,
      selectedFilePath: newActiveTabId ? updatedTabs.find(t => t.id === newActiveTabId)!.path : null
    })
  },
  
  setActiveTab: (tabId) => {
    const tab = get().openTabs.find(t => t.id === tabId)
    if (tab) {
      set({ activeTabId: tabId, selectedFilePath: tab.path })
    }
  },
  
  updateFileContent: (path, content) => {
    set((state) => ({
      fileContents: { ...state.fileContents, [path]: content },
      openTabs: state.openTabs.map(t => t.path === path ? { ...t, isUnsaved: true } : t)
    }))
  }
}))
