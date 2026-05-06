
import { Moon, Sun, Settings, FolderOpen, Play } from 'lucide-react'
import { useUiStore } from '@/stores/ui-store'
import { useProjectStore } from '@/stores/project-store'

export function Toolbar() {
  const { theme, setTheme, setIsSettingsOpen } = useUiStore()
  const { uploadState } = useProjectStore()

  const projectName = uploadState.status === 'ready' ? uploadState.projectName : 'No project'

  return (
    <div className="h-12 border-b border-border bg-background flex items-center justify-between px-4 shrink-0">
      <div className="flex items-center gap-4">
        <div className="font-bold text-lg tracking-tight flex items-center gap-2 text-foreground">
          <Play className="w-5 h-5 text-primary" fill="currentColor" />
          TestIDE
        </div>
      </div>

      <div className="flex items-center gap-2">
        <button 
          onClick={() => document.querySelector<HTMLInputElement>('input[type="file"]')?.click()}
          className="flex items-center gap-2 px-3 py-1.5 text-sm rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80 transition-colors"
        >
          <FolderOpen className="w-4 h-4" />
          {projectName}
        </button>
      </div>

      <div className="flex items-center gap-2">
        <button
          onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
          className="p-2 rounded-md hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
          title="Toggle theme"
        >
          {theme === 'dark' ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
        </button>
        <button
          onClick={() => setIsSettingsOpen(true)}
          className="p-2 rounded-md hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
          title="Settings"
        >
          <Settings className="w-4 h-4" />
        </button>
      </div>
    </div>
  )
}
