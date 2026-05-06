import { Sparkles } from 'lucide-react';

/**
 * Placeholder for the AI action panel (right sidebar). Phase 11 wires
 * `generate_artifact` IPC + a real review queue against
 * `list_artifacts` (which itself needs a new backend command).
 */
export function AiPanelPlaceholder() {
  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-border px-3 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        AI
      </div>
      <div className="flex flex-1 flex-col items-center justify-center p-6 text-center">
        <Sparkles className="text-muted-foreground/50 mb-3 size-8" />
        <h2 className="text-sm font-semibold tracking-tight">Generation panel pending</h2>
        <p className="text-muted-foreground mt-2 max-w-xs text-xs">
          Test plan / cases / defect-report buttons + review queue land in Phase 11. Backend
          `generate_artifact` IPC is already shipped.
        </p>
      </div>
    </div>
  );
}
