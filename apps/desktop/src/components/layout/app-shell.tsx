import type { ReactNode } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';

import { useUiStore, type PanelSizes } from '@/stores/ui-store';

import { StatusBar } from './status-bar';
import { Toolbar } from './toolbar';

type Props = {
  sidebar: ReactNode;
  editor: ReactNode;
  aiPanel: ReactNode;
};

/**
 * Three-panel workspace shell — file explorer | editor | AI panel.
 * Panel sizes persist to localStorage via `useUiStore`. Resize handles
 * are explicit DOM elements so they can be themed without `@ts-ignore`.
 */
export function AppShell({ sidebar, editor, aiPanel }: Props) {
  const panelSizes = useUiStore((s) => s.panelSizes);
  const setPanelSizes = useUiStore((s) => s.setPanelSizes);

  const handleLayout = (sizes: number[]) => {
    if (sizes.length !== 3) return;
    const [a, b, c] = sizes;
    if (a === undefined || b === undefined || c === undefined) return;
    const next: PanelSizes = [a, b, c];
    setPanelSizes(next);
  };

  return (
    <div className="bg-background text-foreground flex h-screen w-screen flex-col overflow-hidden">
      <Toolbar />
      <div className="flex-1 overflow-hidden">
        <PanelGroup direction="horizontal" onLayout={handleLayout}>
          <Panel
            defaultSize={panelSizes[0]}
            minSize={12}
            collapsible
            className="bg-card flex flex-col"
          >
            {sidebar}
          </Panel>
          <ResizeHandle />
          <Panel defaultSize={panelSizes[1]} minSize={30} className="bg-background flex flex-col">
            {editor}
          </Panel>
          <ResizeHandle />
          <Panel
            defaultSize={panelSizes[2]}
            minSize={18}
            collapsible
            className="bg-card flex flex-col"
          >
            {aiPanel}
          </Panel>
        </PanelGroup>
      </div>
      <StatusBar />
    </div>
  );
}

function ResizeHandle() {
  return (
    <PanelResizeHandle className="bg-border hover:bg-primary/50 data-[resize-handle-state=drag]:bg-primary relative w-px transition-colors">
      <div className="absolute inset-y-0 -inset-x-1" aria-hidden="true" />
    </PanelResizeHandle>
  );
}
