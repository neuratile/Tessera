
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
      <div className="flex-1 flex flex-col items-center justify-center bg-background p-8 text-center">
        <div className="max-w-md w-full flex flex-col items-center">
          <div className="w-20 h-20 bg-primary/5 rounded-3xl flex items-center justify-center mb-8 animate-in zoom-in duration-500">
             <svg className="w-10 h-10 text-primary" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
               <path d="M15.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L15.5 2z" />
               <path d="M15 2v6h6" />
               <path d="M9 13h6" />
               <path d="M9 17h3" />
             </svg>
          </div>
          
          <h1 className="text-3xl font-bold tracking-tight mb-3 animate-in fade-in slide-in-from-bottom-4 duration-700 delay-100">TestIDE</h1>
          <p className="text-muted-foreground mb-8 animate-in fade-in slide-in-from-bottom-4 duration-700 delay-200">
            A high-performance environment for generating and reviewing code tests with local AI.
          </p>

          <div className="grid grid-cols-1 gap-3 w-full animate-in fade-in slide-in-from-bottom-4 duration-700 delay-300">
            <button 
              onClick={() => document.querySelector<HTMLInputElement>('input[type="file"]')?.click()}
              className="flex items-center justify-between px-4 py-3 rounded-xl bg-card border border-border hover:border-primary/50 hover:bg-muted/50 transition-all group"
            >
              <div className="flex items-center gap-3">
                <div className="w-8 h-8 rounded-lg bg-blue-500/10 text-blue-500 flex items-center justify-center group-hover:scale-110 transition-transform">
                  <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z"/></svg>
                </div>
                <div className="text-left">
                  <div className="text-sm font-medium">Open Folder</div>
                  <div className="text-xs text-muted-foreground">Select a project to start testing</div>
                </div>
              </div>
              <span className="text-xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded opacity-0 group-hover:opacity-100 transition-opacity">Ctrl+O</span>
            </button>

            <div className="mt-8 pt-8 border-t border-border/50 w-full">
              <div className="text-xs font-medium text-muted-foreground uppercase tracking-widest mb-4">Recent Projects</div>
              <div className="text-sm text-muted-foreground italic">No recent projects found</div>
            </div>
          </div>
        </div>
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
