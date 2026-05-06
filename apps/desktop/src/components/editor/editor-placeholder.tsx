import { FileText } from 'lucide-react';

import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Placeholder for the editor pane. Phase 10 will replace this with a
 * Monaco-based tab editor. For now we surface the selected file path
 * and a hint about where syntax-highlighted content will land.
 */
export function EditorPlaceholder() {
  const selected = useWorkspaceStore((s) => s.selectedPath);

  if (selected === null) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <FileText className="text-muted-foreground/50 mb-3 size-10" />
        <h2 className="text-lg font-semibold tracking-tight">No file selected</h2>
        <p className="text-muted-foreground mt-1 max-w-md text-sm">
          Pick a file in the explorer to preview it. Monaco-backed editing lands in the next phase.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 items-center justify-center p-8 text-center">
      <div className="flex max-w-md flex-col items-center">
        <FileText className="text-muted-foreground mb-3 size-10" />
        <h2 className="text-lg font-semibold tracking-tight">Selected</h2>
        <code className="text-muted-foreground bg-muted mt-2 max-w-full truncate rounded px-1.5 py-0.5 text-xs">
          {selected}
        </code>
        <p className="text-muted-foreground mt-3 text-xs">
          Editor lands in Phase 10 (Monaco + tabs). File contents are not loaded yet.
        </p>
      </div>
    </div>
  );
}
