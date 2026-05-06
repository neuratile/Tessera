
import { useProjectStore } from '@/stores/project-store'
import { useAiStore } from '@/stores/ai-store'

export function StatusBar() {
  const { selectedFilePath } = useProjectStore()
  const { provider, selectedModel } = useAiStore()

  return (
    <div className="h-6 border-t border-border bg-background flex items-center justify-between px-3 shrink-0 text-xs text-muted-foreground select-none">
      <div className="flex items-center">
        {selectedFilePath ? (
          <span className="truncate max-w-md">{selectedFilePath}</span>
        ) : (
          <span>Ready</span>
        )}
      </div>

      <div className="flex items-center gap-4">
        <div className="flex items-center gap-1.5">
          <div className="w-2 h-2 rounded-full bg-green-500" />
          <span>Connected</span>
        </div>
        <div className="flex items-center gap-1">
          <span className="font-medium">{provider}</span>
          {selectedModel && <span className="opacity-70">({selectedModel})</span>}
        </div>
      </div>
    </div>
  )
}
