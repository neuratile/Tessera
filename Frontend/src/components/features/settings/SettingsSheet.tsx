import { useState } from 'react'
import { X, Server, Key, Bot } from 'lucide-react'
import { useUiStore } from '@/stores/ui-store'
import { useAiStore, type AiProvider } from '@/stores/ai-store'
import { cn } from '@/lib/utils'

export function SettingsSheet() {
  const { isSettingsOpen, setIsSettingsOpen } = useUiStore()
  
  if (!isSettingsOpen) return null

  return (
    <>
      {/* Backdrop */}
      <div 
        className="fixed inset-0 bg-background/80 backdrop-blur-sm z-50 transition-opacity"
        onClick={() => setIsSettingsOpen(false)}
      />
      
      {/* Sheet */}
      <div className="fixed inset-y-0 right-0 w-full max-w-md bg-background border-l border-border shadow-2xl z-50 flex flex-col transform transition-transform duration-300 ease-in-out">
        <div className="flex items-center justify-between p-4 border-b border-border">
          <h2 className="text-lg font-semibold tracking-tight">Settings</h2>
          <button 
            onClick={() => setIsSettingsOpen(false)}
            className="p-1.5 rounded-md hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>
        
        <div className="flex-1 overflow-y-auto p-6">
          <AiProviderPage />
        </div>
      </div>
    </>
  )
}

function AiProviderPage() {
  const { provider, apiKey, baseUrl, selectedModel, setProviderConfig } = useAiStore()
  const [testing, setTesting] = useState(false)
  const [testResult, setTestResult] = useState<'success' | 'error' | null>(null)

  const handleTestConnection = () => {
    setTesting(true)
    setTestResult(null)
    setTimeout(() => {
      setTesting(false)
      setTestResult('success')
    }, 1000)
  }

  const providers: { id: AiProvider; name: string; icon: React.ReactNode }[] = [
    { id: 'ollama', name: 'Ollama (Local)', icon: <Server className="w-4 h-4" /> },
    { id: 'openai', name: 'OpenAI', icon: <Bot className="w-4 h-4" /> },
    { id: 'anthropic', name: 'Anthropic', icon: <Bot className="w-4 h-4" /> },
    { id: 'openrouter', name: 'OpenRouter', icon: <Bot className="w-4 h-4" /> },
  ]

  return (
    <div className="flex flex-col gap-8">
      <section className="space-y-4">
        <div>
          <h3 className="text-base font-medium mb-1 text-foreground">AI Provider</h3>
          <p className="text-sm text-muted-foreground">Select the backend service to generate your tests.</p>
        </div>
        
        <div className="grid grid-cols-2 gap-3">
          {providers.map((p) => (
            <label 
              key={p.id}
              className={cn(
                "flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors text-sm font-medium",
                provider === p.id 
                  ? "border-primary bg-primary/5 text-primary" 
                  : "border-border bg-card text-muted-foreground hover:bg-muted/50 hover:text-foreground"
              )}
            >
              <input 
                type="radio" 
                name="provider" 
                value={p.id} 
                checked={provider === p.id}
                onChange={(e) => setProviderConfig({ provider: e.target.value as AiProvider })}
                className="hidden"
              />
              {p.icon}
              {p.name}
            </label>
          ))}
        </div>
      </section>

      <section className="space-y-4">
        <h3 className="text-base font-medium text-foreground">Connection Details</h3>
        
        {provider === 'ollama' ? (
          <div className="space-y-2">
            <label className="text-sm font-medium text-foreground">Base URL</label>
            <input 
              type="text" 
              value={baseUrl}
              onChange={(e) => setProviderConfig({ baseUrl: e.target.value })}
              className="w-full bg-background border border-border rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/50"
            />
          </div>
        ) : (
          <div className="space-y-2">
            <label className="text-sm font-medium text-foreground">API Key</label>
            <div className="relative">
              <Key className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
              <input 
                type="password" 
                value={apiKey}
                onChange={(e) => setProviderConfig({ apiKey: e.target.value })}
                placeholder="sk-..."
                className="w-full bg-background border border-border rounded-md pl-9 pr-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/50"
              />
            </div>
          </div>
        )}
        
        <div className="space-y-2">
          <label className="text-sm font-medium text-foreground">Model Name</label>
          <input 
            type="text" 
            value={selectedModel}
            onChange={(e) => setProviderConfig({ selectedModel: e.target.value })}
            placeholder={provider === 'ollama' ? 'llama3:latest' : 'gpt-4o'}
            className="w-full bg-background border border-border rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/50"
          />
        </div>
      </section>

      <div className="pt-4 border-t border-border flex items-center justify-between">
        <div className="text-sm">
          {testResult === 'success' && <span className="text-green-500 font-medium">Connection successful!</span>}
          {testResult === 'error' && <span className="text-red-500 font-medium">Connection failed.</span>}
        </div>
        <button 
          onClick={handleTestConnection}
          disabled={testing}
          className="bg-secondary text-secondary-foreground hover:bg-secondary/80 px-4 py-2 rounded-md text-sm font-medium transition-colors"
        >
          {testing ? 'Testing...' : 'Test Connection'}
        </button>
      </div>
    </div>
  )
}
