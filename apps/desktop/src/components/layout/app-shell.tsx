import { useCallback, useRef, type ReactNode } from 'react';
import {
  type ImperativePanelHandle,
  Panel,
  PanelGroup,
  PanelResizeHandle,
} from 'react-resizable-panels';

import { COMMAND, useCommand } from '@/lib/command-bus';
import { useUiStore, type PanelSizes } from '@/stores/ui-store';

import { BoardPanel } from '@/components/boards/board-panel';
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
 *
 * The sidebar and AI panel can be toggled hidden via the View menu
 * (`Cmd/Ctrl+B`, `Cmd/Ctrl+J`) or the equivalent keyboard shortcuts.
 * Toggling goes through `ImperativePanelHandle.collapse()` /
 * `.expand()` so `react-resizable-panels` re-balances the editor
 * width on its own and `onLayout` fires the new sizes through the
 * store for persistence.
 */
export function AppShell({ sidebar, editor, aiPanel }: Props) {
  const panelSizes = useUiStore((s) => s.panelSizes);
  const setPanelSizes = useUiStore((s) => s.setPanelSizes);
  const mode = useUiStore((s) => s.mode);

  const sidebarRef = useRef<ImperativePanelHandle | null>(null);
  const aiPanelRef = useRef<ImperativePanelHandle | null>(null);

  const handleLayout = (sizes: number[]) => {
    if (sizes.length !== 3) return;
    const [a, b, c] = sizes;
    if (a === undefined || b === undefined || c === undefined) return;
    const next: PanelSizes = [a, b, c];
    setPanelSizes(next);
  };

  const togglePanel = useCallback((handle: ImperativePanelHandle | null) => {
    if (handle === null) return;
    if (handle.isCollapsed()) {
      handle.expand();
    } else {
      handle.collapse();
    }
  }, []);

  useCommand(
    COMMAND.ViewToggleSidebar,
    useCallback(() => togglePanel(sidebarRef.current), [togglePanel]),
  );
  useCommand(
    COMMAND.ViewToggleAiPanel,
    useCallback(() => togglePanel(aiPanelRef.current), [togglePanel]),
  );

  return (
    <div className="bg-background text-foreground flex h-screen w-screen flex-col overflow-hidden">
      <Toolbar />
      <div className="flex-1 overflow-hidden">
        {mode === 'boards' ? (
          <BoardPanel />
        ) : (
          <PanelGroup direction="horizontal" onLayout={handleLayout}>
            <Panel
              ref={sidebarRef}
              defaultSize={panelSizes[0]}
              minSize={12}
              collapsible
              collapsedSize={0}
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
              ref={aiPanelRef}
              defaultSize={panelSizes[2]}
              minSize={18}
              collapsible
              collapsedSize={0}
              className="bg-card flex flex-col"
            >
              {aiPanel}
            </Panel>
          </PanelGroup>
        )}
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
