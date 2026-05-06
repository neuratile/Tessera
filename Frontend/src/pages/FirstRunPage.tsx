import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Check, Cpu, HardDrive, TerminalSquare, ArrowRight } from 'lucide-react'
import { cn } from '@/lib/utils'

export function FirstRunPage() {
  const [step, setStep] = useState(1)
  const navigate = useNavigate()

  const handleFinish = () => {
    localStorage.setItem("ide.firstRunComplete", "true")
    navigate("/ide")
  }

  return (
    <div className="h-screen w-screen bg-background text-foreground flex items-center justify-center p-4">
      <div className="max-w-2xl w-full bg-card border border-border shadow-2xl rounded-2xl overflow-hidden flex flex-col h-[500px]">
        {/* Header */}
        <div className="p-6 border-b border-border bg-muted/20 shrink-0">
          <div className="flex items-center gap-2 text-primary font-bold text-xl mb-4">
            <TerminalSquare className="w-6 h-6" />
            TestIDE Setup
          </div>
          
          {/* Progress Steps */}
          <div className="flex items-center justify-between gap-2">
            {[1, 2, 3, 4].map((s) => (
              <div key={s} className="flex-1 flex items-center gap-2">
                <div className={cn(
                  "h-2 flex-1 rounded-full transition-colors",
                  step >= s ? "bg-primary" : "bg-muted"
                )} />
              </div>
            ))}
          </div>
        </div>

        {/* Body */}
        <div className="flex-1 p-8 overflow-y-auto">
          {step === 1 && <Step1 />}
          {step === 2 && <Step2 />}
          {step === 3 && <Step3 />}
          {step === 4 && <Step4 />}
        </div>

        {/* Footer */}
        <div className="p-6 border-t border-border bg-muted/20 flex justify-between shrink-0">
          <button
            onClick={() => setStep(s => Math.max(1, s - 1))}
            disabled={step === 1}
            className="px-4 py-2 rounded-md text-sm font-medium text-muted-foreground hover:text-foreground disabled:opacity-0 transition-colors"
          >
            Back
          </button>
          
          {step < 4 ? (
            <button
              onClick={() => setStep(s => s + 1)}
              className="px-6 py-2 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90 transition-colors flex items-center gap-2"
            >
              Continue <ArrowRight className="w-4 h-4" />
            </button>
          ) : (
            <button
              onClick={handleFinish}
              className="px-6 py-2 rounded-md text-sm font-medium bg-green-600 text-white hover:bg-green-700 transition-colors flex items-center gap-2"
            >
              Launch IDE <Check className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

function Step1() {
  return (
    <div className="flex flex-col gap-4 animate-in fade-in slide-in-from-bottom-4">
      <h2 className="text-2xl font-bold tracking-tight">Welcome to TestIDE</h2>
      <p className="text-muted-foreground">
        A local-first, AI-powered IDE specifically designed for generating and reviewing tests for your codebase.
      </p>
      <div className="mt-4 p-4 border border-border rounded-lg bg-background flex flex-col gap-3 text-sm">
        <div className="flex items-center gap-2 text-foreground">
          <Check className="w-4 h-4 text-green-500" /> Fully local operations by default
        </div>
        <div className="flex items-center gap-2 text-foreground">
          <Check className="w-4 h-4 text-green-500" /> Supports multiple languages & frameworks
        </div>
        <div className="flex items-center gap-2 text-foreground">
          <Check className="w-4 h-4 text-green-500" /> Easy review and export workflow
        </div>
      </div>
    </div>
  )
}

function Step2() {
  return (
    <div className="flex flex-col gap-4 animate-in fade-in slide-in-from-bottom-4">
      <h2 className="text-2xl font-bold tracking-tight">Hardware Detection</h2>
      <p className="text-muted-foreground">
        We've analyzed your system to recommend the best AI models for local execution.
      </p>
      
      <div className="grid grid-cols-2 gap-4 mt-4">
        <div className="border border-border rounded-lg p-4 bg-background flex items-start gap-3">
          <Cpu className="w-5 h-5 text-primary mt-0.5" />
          <div>
            <div className="text-sm font-medium">Processor</div>
            <div className="text-xs text-muted-foreground mt-1">Apple M3 Max (Mock)</div>
            <div className="text-xs text-green-500 mt-1">Excellent for local AI</div>
          </div>
        </div>
        <div className="border border-border rounded-lg p-4 bg-background flex items-start gap-3">
          <HardDrive className="w-5 h-5 text-primary mt-0.5" />
          <div>
            <div className="text-sm font-medium">Memory</div>
            <div className="text-xs text-muted-foreground mt-1">36 GB Unified RAM</div>
            <div className="text-xs text-green-500 mt-1">Can run 14B models</div>
          </div>
        </div>
      </div>
    </div>
  )
}

function Step3() {
  return (
    <div className="flex flex-col gap-4 animate-in fade-in slide-in-from-bottom-4">
      <h2 className="text-2xl font-bold tracking-tight">Local AI Engine</h2>
      <p className="text-muted-foreground">
        TestIDE uses Ollama to run models entirely on your machine. No code leaves your computer.
      </p>
      
      <div className="mt-4 border border-border rounded-lg p-6 bg-background flex flex-col items-center justify-center text-center gap-4">
        <div className="w-12 h-12 bg-primary/10 text-primary rounded-full flex items-center justify-center">
          <TerminalSquare className="w-6 h-6" />
        </div>
        <div>
          <div className="font-medium">Ollama is required</div>
          <div className="text-sm text-muted-foreground mt-1 max-w-sm">
            Please install Ollama from ollama.ai, start the app, and then continue.
          </div>
        </div>
        <div className="flex items-center gap-2 mt-2 px-3 py-1.5 bg-green-500/10 text-green-600 dark:text-green-400 rounded-full text-xs font-medium">
          <Check className="w-3.5 h-3.5" /> Detected running on localhost:11434
        </div>
      </div>
    </div>
  )
}

function Step4() {
  return (
    <div className="flex flex-col gap-4 animate-in fade-in slide-in-from-bottom-4">
      <h2 className="text-2xl font-bold tracking-tight">Select Base Model</h2>
      <p className="text-muted-foreground">
        Choose a model to start with. You can change this later in Settings or use cloud providers.
      </p>
      
      <div className="flex flex-col gap-3 mt-4">
        <label className="border-2 border-primary bg-primary/5 rounded-lg p-4 cursor-pointer flex items-center justify-between">
          <div>
            <div className="font-medium flex items-center gap-2">
              Qwen 2.5 Coder (7B) <span className="text-[10px] bg-primary text-primary-foreground px-1.5 py-0.5 rounded-sm uppercase tracking-wider">Recommended</span>
            </div>
            <div className="text-xs text-muted-foreground mt-1">Fast, accurate, excellent for test generation. ~4GB RAM</div>
          </div>
          <input type="radio" name="model" defaultChecked className="hidden" />
          <div className="w-5 h-5 rounded-full border-4 border-primary bg-background" />
        </label>
        
        <label className="border border-border bg-background hover:bg-muted/50 rounded-lg p-4 cursor-pointer flex items-center justify-between transition-colors">
          <div>
            <div className="font-medium">DeepSeek Coder V2 (16B)</div>
            <div className="text-xs text-muted-foreground mt-1">More capable, slower generation. ~10GB RAM</div>
          </div>
          <input type="radio" name="model" className="hidden" />
          <div className="w-5 h-5 rounded-full border border-border" />
        </label>
      </div>
    </div>
  )
}
