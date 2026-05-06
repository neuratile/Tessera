import { type ReactNode, useEffect } from 'react'
// @ts-ignore
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels'
import { Toolbar } from './Toolbar'
import { StatusBar } from './StatusBar'
import { useUiStore } from '@/stores/ui-store'
import { cn } from '@/lib/utils'

interface AppShellProps {
  sidebar: ReactNode
  editor: ReactNode
  aiPanel: ReactNode
}

export function AppShell({ sidebar, editor, aiPanel }: AppShellProps) {
  const { theme, panelSizes, setPanelSizes } = useUiStore()

  useEffect(() => {
    if (theme === 'dark') {
      document.documentElement.classList.add('dark')
      document.documentElement.classList.remove('light')
    } else {
      document.documentElement.classList.add('light')
      document.documentElement.classList.remove('dark')
    }
  }, [theme])

  const handleLayout = (sizes: number[]) => {
    setPanelSizes(sizes)
  }

  return (
    <div className="h-screen w-screen flex flex-col overflow-hidden bg-background text-foreground">
      <Toolbar />
      
      <div className="flex-1 overflow-hidden">
        {/* @ts-ignore */}
        <PanelGroup direction="horizontal" onLayout={handleLayout}>
          {/* @ts-ignore */}
          <Panel 
            defaultSize={panelSizes[0]} 
            minSize={15} 
            collapsible 
            className="flex flex-col bg-card"
          >
            {sidebar}
          </Panel>
          
          <ResizeHandle />
          
          {/* @ts-ignore */}
          <Panel 
            defaultSize={panelSizes[1]} 
            minSize={30} 
            className="flex flex-col bg-background"
          >
            {editor}
          </Panel>
          
          <ResizeHandle />
          
          {/* @ts-ignore */}
          <Panel 
            defaultSize={panelSizes[2]} 
            minSize={20} 
            collapsible 
            className="flex flex-col bg-card"
          >
            {aiPanel}
          </Panel>
        </PanelGroup>
      </div>

      <StatusBar />
    </div>
  )
}

function ResizeHandle({ className = "" }: { className?: string }) {
  return (
    // @ts-ignore
    <PanelResizeHandle className={cn("w-1 bg-border/50 hover:bg-primary/50 transition-colors cursor-col-resize z-10", className)}>
      <div className="h-full w-full flex items-center justify-center">
        <div className="h-8 w-[2px] bg-muted-foreground/30 rounded-full" />
      </div>
    </PanelResizeHandle>
  )
}
