import { useState } from 'react'
import { Sparkles, Loader2, Check, X, RefreshCw, Download } from 'lucide-react'
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

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden border-l border-border/50">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50 shrink-0">
        <h2 className="font-semibold tracking-tight text-foreground flex items-center gap-2">
          <Sparkles className="w-4 h-4 text-indigo-400" />
          AI Actions
        </h2>
        <button
          onClick={handleExport}
          disabled={reviewQueue.filter(i => i.status === 'approved').length === 0}
          className="p-1.5 rounded-md hover:bg-muted text-muted-foreground hover:text-foreground disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          title="Export Approved Tests"
        >
          <Download className="w-4 h-4" />
        </button>
      </div>

      <div className="p-4 flex flex-col gap-4 shrink-0 border-b border-border/50 bg-muted/10">
        <div className="space-y-3">
          <label className="text-xs font-medium text-muted-foreground uppercase tracking-wider">Scope</label>
          <div className="flex gap-4 text-sm">
            <label className="flex items-center gap-2 cursor-pointer text-foreground">
              <input 
                type="radio" 
                name="scope" 
                value="current" 
                checked={scope === 'current'}
                onChange={() => setScope('current')}
                className="accent-primary"
              />
              Current File
            </label>
            <label className="flex items-center gap-2 cursor-pointer text-foreground">
              <input 
                type="radio" 
                name="scope" 
                value="all" 
                checked={scope === 'all'}
                onChange={() => setScope('all')}
                className="accent-primary"
              />
              All Files
            </label>
          </div>
        </div>

        <button
          onClick={handleGenerate}
          disabled={!isReady || isGenerating}
          className={cn(
            "w-full py-2.5 rounded-md font-medium flex items-center justify-center gap-2 transition-all relative overflow-hidden",
            isGenerating 
              ? "bg-primary/20 text-primary cursor-default" 
              : "bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
          )}
        >
          {isGenerating ? (
            <>
              <div 
                className="absolute left-0 top-0 bottom-0 bg-primary/20 transition-all duration-300 ease-linear"
                style={{ width: `${generationState.status === 'streaming' ? generationState.progress : 0}%` }}
              />
              <Loader2 className="w-4 h-4 animate-spin relative z-10" />
              <span className="relative z-10">Generating tests...</span>
            </>
          ) : (
            <>
              <Sparkles className="w-4 h-4" />
              Generate Tests
            </>
          )}
        </button>

        {generationState.status === 'streaming' && (
          <div className="text-xs text-muted-foreground truncate animate-pulse">
            Analyzing {generationState.currentFile}...
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
