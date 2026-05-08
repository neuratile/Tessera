import { Loader2 } from 'lucide-react';

import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Bottom status bar. Surfaces project status, analysis pipeline
 * progress, the selected file, and the latest tree-load / analysis
 * error.
 */
export function StatusBar() {
  const project = useWorkspaceStore((s) => s.project);
  const selectedPath = useWorkspaceStore((s) => s.selectedPath);
  const treeError = useWorkspaceStore((s) => s.treeError);
  const analysis = useWorkspaceStore((s) => s.analysis);

  return (
    <footer className="flex h-7 shrink-0 items-center justify-between gap-2 border-t border-border bg-surface-3 px-3 font-mono text-[11px] text-muted-foreground">
      <div className="flex items-center gap-4">
        {project ? (
          <>
            <span className="flex items-center gap-1" data-testid="project-status">
              <span className="size-1.5 rounded-full bg-primary" aria-hidden="true" />
              {project.status}
            </span>
            <span>{project.fileCount} files</span>
          </>
        ) : (
          <span>no project</span>
        )}
        {analysis.status === 'pending' ? (
          <span
            className="flex items-center gap-1 text-muted-foreground"
            data-testid="analysis-status"
          >
            <Loader2 className="size-3 animate-spin" />
            analysing…
          </span>
        ) : analysis.status === 'ready' ? (
          <span className="text-muted-foreground" data-testid="analysis-status">
            {analysis.outcome.chunksEmbedded} chunks · {analysis.outcome.filesParsed} parsed
          </span>
        ) : analysis.status === 'error' ? (
          <span
            className="text-destructive truncate"
            role="alert"
            title={analysis.message}
            data-testid="analysis-status"
          >
            analysis failed
          </span>
        ) : null}
      </div>
      <div className="flex items-center gap-3">
        {treeError !== null ? (
          <span className="text-destructive truncate" role="alert" title={treeError}>
            {treeError}
          </span>
        ) : null}
        {selectedPath !== null ? (
          <code className="text-muted-foreground truncate">{selectedPath}</code>
        ) : null}
      </div>
    </footer>
  );
}
