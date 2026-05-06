import { useState } from 'react'
import { Sparkles, Loader2, Check, X, RefreshCw, Download, TerminalSquare, CheckSquare, Code2, Layers, SearchCode, ShieldAlert } from 'lucide-react'
import { useAiStore, type ReviewItem } from '@/stores/ai-store'
import { useProjectStore } from '@/stores/project-store'
import { cn } from '@/lib/utils'

export function AiActionPanel() {
  const { generationState, reviewQueue, updateReviewItem } = useAiStore()
  const { uploadState, selectedFilePath } = useProjectStore()
  const [scope, setScope] = useState<'current' | 'all'>('current')

  const isGenerating = generationState.status === 'streaming'
  const isReady = uploadState.status === 'ready'

  const handleGenerate = () => {
    if (!isReady || isGenerating) return
    // Mocking API call for generation
    useAiStore.setState({ generationState: { status: 'streaming', progress: 0, currentFile: selectedFilePath || 'unknown' } })
    
    // Simulate progress
    let progress = 0
    const interval = setInterval(() => {
      progress += 10
      useAiStore.setState({ generationState: { status: 'streaming', progress, currentFile: selectedFilePath || 'unknown' } })
      
      if (progress >= 100) {
        clearInterval(interval)
        useAiStore.setState({ generationState: { status: 'done', generatedCount: 1 } })
        
        // Add mock review item
        const newItem: ReviewItem = {
          id: Math.random().toString(36).substring(7),
          filePath: selectedFilePath || 'src/utils.ts',
          generatedTest: "import { test, expect } from 'vitest'\n\ntest('mock test', () => {\n  expect(1).toBe(1)\n})",
          status: 'pending',
          feedback: ''
        }
        useAiStore.setState((s) => ({ reviewQueue: [newItem, ...s.reviewQueue] }))
      }
    }, 300)
  }

  const handleExport = () => {
    // Export approved tests logic
    const approved = reviewQueue.filter(item => item.status === 'approved')
    if (approved.length === 0) return
    
    let content = '# Generated Tests\n\n'
    approved.forEach(item => {
      content += `## ${item.filePath}\n\n\`\`\`typescript\n${item.generatedTest}\n\`\`\`\n\n`
    })
    
    const blob = new Blob([content], { type: 'text/markdown' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'tests.md'
    a.click()
    URL.revokeObjectURL(url)
  }

  const actions = [
    { id: 'plan', label: 'Test Plan', icon: <TerminalSquare className="w-4 h-4" />, color: 'text-blue-400', bg: 'bg-blue-400/10' },
    { id: 'cases', label: 'Test Cases', icon: <CheckSquare className="w-4 h-4" />, color: 'text-emerald-400', bg: 'bg-emerald-400/10' },
    { id: 'unit', label: 'Unit Tests', icon: <Code2 className="w-4 h-4" />, color: 'text-amber-400', bg: 'bg-amber-400/10' },
    { id: 'integration', label: 'Integration', icon: <Layers className="w-4 h-4" />, color: 'text-purple-400', bg: 'bg-purple-400/10' },
    { id: 'review', label: 'Code Review', icon: <SearchCode className="w-4 h-4" />, color: 'text-rose-400', bg: 'bg-rose-400/10' },
    { id: 'security', label: 'Security', icon: <ShieldAlert className="w-4 h-4" />, color: 'text-indigo-400', bg: 'bg-indigo-400/10' },
  ]

  const handleAction = (actionId: string) => {
    if (!isReady || isGenerating) return
    handleGenerate()
  }

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden border-l border-border/50">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50 shrink-0 bg-muted/20">
        <h2 className="font-semibold tracking-tight text-foreground flex items-center gap-2">
          <div className="w-2 h-2 rounded-full bg-indigo-500 animate-pulse" />
          AI Core
        </h2>
        <div className="flex items-center gap-1">
          <button
            onClick={handleExport}
            disabled={reviewQueue.filter(i => i.status === 'approved').length === 0}
            className="p-1.5 rounded-md hover:bg-muted text-muted-foreground hover:text-foreground disabled:opacity-50 transition-colors"
            title="Export Approved"
          >
            <Download className="w-4 h-4" />
          </button>
        </div>
      </div>

      <div className="p-4 flex flex-col gap-5 shrink-0 border-b border-border/50">
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <label className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest">Active Scope</label>
            <span className="text-[10px] font-medium text-primary px-1.5 py-0.5 rounded bg-primary/10">Ollama Running</span>
          </div>
          <div className="flex p-1 bg-muted/50 rounded-lg border border-border/50">
            <button 
              onClick={() => setScope('current')}
              className={cn(
                "flex-1 py-1.5 text-xs font-medium rounded-md transition-all",
                scope === 'current' ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground"
              )}
            >
              Current File
            </button>
            <button 
              onClick={() => setScope('all')}
              className={cn(
                "flex-1 py-1.5 text-xs font-medium rounded-md transition-all",
                scope === 'all' ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground"
              )}
            >
              All Files
            </button>
          </div>
        </div>

        <div className="space-y-3">
          <label className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest">Quick Actions</label>
          <div className="grid grid-cols-2 gap-2">
            {actions.map((action) => (
              <button
                key={action.id}
                onClick={() => handleAction(action.id)}
                disabled={!isReady || isGenerating}
                className={cn(
                  "flex flex-col items-center justify-center gap-2 p-3 rounded-xl border border-border/50 hover:border-primary/30 transition-all group relative overflow-hidden",
                  isGenerating ? "opacity-50 cursor-not-allowed" : "hover:bg-muted/30"
                )}
              >
                <div className={cn("p-2 rounded-lg group-hover:scale-110 transition-transform", action.bg, action.color)}>
                  {action.icon}
                </div>
                <span className="text-[10px] font-semibold text-foreground truncate w-full text-center">{action.label}</span>
              </button>
            ))}
          </div>
        </div>

        {isGenerating && (
          <div className="space-y-2 animate-in fade-in slide-in-from-top-2">
            <div className="h-1.5 w-full bg-muted rounded-full overflow-hidden">
              <div 
                className="h-full bg-primary transition-all duration-300 ease-linear"
                style={{ width: `${generationState.status === 'streaming' ? generationState.progress : 0}%` }}
              />
            </div>
            <div className="flex items-center justify-between text-[10px] text-muted-foreground">
              <span className="truncate flex-1">Analyzing {generationState.currentFile}...</span>
              <span className="font-mono">{generationState.status === 'streaming' ? generationState.progress : 0}%</span>
            </div>
          </div>
        )}
      </div>

      {/* Review Queue */}
      <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-3">
        <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-1">
          Review Queue ({reviewQueue.length})
        </h3>
        
        {reviewQueue.length === 0 ? (
          <div className="text-sm text-muted-foreground text-center py-8">
            Generated tests will appear here for review.
          </div>
        ) : (
          reviewQueue.map((item) => (
            <ReviewCard key={item.id} item={item} onUpdate={updateReviewItem} />
          ))
        )}
      </div>
    </div>
  )
}

function ReviewCard({ item, onUpdate }: { item: ReviewItem; onUpdate: (id: string, updates: Partial<ReviewItem>) => void }) {
  return (
    <div className={cn(
      "border rounded-lg p-3 flex flex-col gap-2 text-sm transition-colors",
      item.status === 'approved' ? "border-green-500/30 bg-green-500/5" :
      item.status === 'rejected' ? "border-red-500/30 bg-red-500/5" :
      "border-border bg-card"
    )}>
      <div className="flex items-center justify-between">
        <span className="font-medium truncate" title={item.filePath}>{item.filePath}</span>
        <StatusBadge status={item.status} />
      </div>
      
      <div className="bg-muted rounded text-xs p-2 overflow-x-auto max-h-32 hide-scrollbar font-mono text-muted-foreground">
        <pre>{item.generatedTest}</pre>
      </div>

      {item.status === 'pending' && (
        <div className="flex items-center gap-2 mt-1">
          <button
            onClick={() => onUpdate(item.id, { status: 'approved' })}
            className="flex-1 bg-green-500/10 text-green-600 dark:text-green-400 hover:bg-green-500/20 py-1.5 rounded flex items-center justify-center gap-1.5 transition-colors"
          >
            <Check className="w-3.5 h-3.5" /> Approve
          </button>
          <button
            onClick={() => onUpdate(item.id, { status: 'rejected' })}
            className="flex-1 bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-500/20 py-1.5 rounded flex items-center justify-center gap-1.5 transition-colors"
          >
            <X className="w-3.5 h-3.5" /> Reject
          </button>
          <button
            onClick={() => onUpdate(item.id, { status: 'regenerating' })}
            className="px-2 bg-muted hover:bg-muted/80 py-1.5 rounded flex items-center justify-center text-foreground transition-colors"
            title="Regenerate"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>
      )}
      
      {(item.status === 'rejected' || item.status === 'regenerating') && (
        <div className="mt-2 flex flex-col gap-2">
          <textarea 
            placeholder="Provide feedback for regeneration..."
            className="w-full text-xs bg-background border border-border rounded p-2 focus:outline-none focus:ring-1 focus:ring-primary resize-none min-h-[60px]"
            value={item.feedback}
            onChange={(e) => onUpdate(item.id, { feedback: e.target.value })}
          />
          <button 
            onClick={() => {
              // Mock regenerate
              onUpdate(item.id, { status: 'pending' })
            }}
            className="bg-primary text-primary-foreground py-1.5 rounded text-xs font-medium"
          >
            Submit & Regenerate
          </button>
        </div>
      )}
    </div>
  )
}

function StatusBadge({ status }: { status: string }) {
  if (status === 'approved') return <span className="text-[10px] uppercase font-bold text-green-500 px-1.5 py-0.5 rounded bg-green-500/10 tracking-wider">Approved</span>
  if (status === 'rejected') return <span className="text-[10px] uppercase font-bold text-red-500 px-1.5 py-0.5 rounded bg-red-500/10 tracking-wider">Rejected</span>
  if (status === 'regenerating') return <span className="text-[10px] uppercase font-bold text-amber-500 px-1.5 py-0.5 rounded bg-amber-500/10 tracking-wider flex items-center gap-1"><RefreshCw className="w-3 h-3 animate-spin"/> Regen</span>
  return <span className="text-[10px] uppercase font-bold text-blue-500 px-1.5 py-0.5 rounded bg-blue-500/10 tracking-wider">Pending</span>
}
