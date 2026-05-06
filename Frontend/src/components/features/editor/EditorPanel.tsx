
import Editor from '@monaco-editor/react'
import { X, Circle } from 'lucide-react'
import { useProjectStore } from '@/stores/project-store'
import { useUiStore } from '@/stores/ui-store'
import { cn } from '@/lib/utils'

export function EditorPanel() {
  const { openTabs, activeTabId, fileContents, closeTab, setActiveTab, updateFileContent } = useProjectStore()
  const { theme } = useUiStore()
  
  const activeTab = openTabs.find(t => t.id === activeTabId)
  const monacoTheme = theme === 'dark' ? 'vs-dark' : 'light'

  if (openTabs.length === 0 || !activeTab) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground bg-background">
        <div className="w-24 h-24 mb-4 opacity-20">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
            <path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z" />
            <polyline points="14 2 14 8 20 8" />
          </svg>
        </div>
        <p className="text-sm">Select a file to start coding</p>
      </div>
    )
  }

  const handleEditorChange = (value: string | undefined) => {
    if (value !== undefined) {
      updateFileContent(activeTab.path, value)
    }
  }

  const language = getLanguageFromFilename(activeTab.name)

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Tabs */}
      <div className="flex overflow-x-auto bg-muted/30 border-b border-border/50 hide-scrollbar shrink-0">
        {openTabs.map(tab => (
          <div
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={cn(
              "flex items-center gap-2 px-3 py-2 text-sm cursor-pointer border-r border-border/50 transition-colors min-w-fit max-w-[200px]",
              activeTabId === tab.id 
                ? "bg-background text-foreground border-t-2 border-t-primary" 
                : "text-muted-foreground hover:bg-muted/50 border-t-2 border-t-transparent"
            )}
          >
            <span className="truncate flex-1">{tab.name}</span>
            <button
              onClick={(e) => {
                e.stopPropagation()
                closeTab(tab.id)
              }}
              className="p-0.5 rounded-sm hover:bg-muted/80 text-muted-foreground hover:text-foreground shrink-0"
            >
              {tab.isUnsaved ? (
                <Circle className="w-3 h-3 fill-current text-primary" />
              ) : (
                <X className="w-3 h-3" />
              )}
            </button>
          </div>
        ))}
      </div>

      {/* Editor Content */}
      <div className="flex-1 relative">
        <Editor
          height="100%"
          language={language}
          theme={monacoTheme}
          value={fileContents[activeTab.path] || ''}
          onChange={handleEditorChange}
          options={{
            minimap: { enabled: false },
            fontSize: 14,
            fontFamily: '"JetBrains Mono", monospace',
            wordWrap: 'on',
            lineNumbersMinChars: 3,
            scrollBeyondLastLine: false,
            smoothScrolling: true,
            cursorBlinking: 'smooth',
            cursorSmoothCaretAnimation: 'on',
            formatOnPaste: true,
          }}
          loading={
            <div className="flex items-center justify-center h-full text-muted-foreground">
              Loading editor...
            </div>
          }
        />
      </div>
    </div>
  )
}

function getLanguageFromFilename(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase()
  switch (ext) {
    case 'ts':
    case 'tsx':
      return 'typescript'
    case 'js':
    case 'jsx':
      return 'javascript'
    case 'json':
      return 'json'
    case 'html':
      return 'html'
    case 'css':
      return 'css'
    case 'rs':
      return 'rust'
    case 'py':
      return 'python'
    case 'md':
      return 'markdown'
    default:
      return 'plaintext'
  }
}
