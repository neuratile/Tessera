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
    <footer className="flex h-6 shrink-0 items-center justify-between gap-2 border-t border-border bg-card px-3 text-xs">
      <div className="flex items-center gap-3">
        {project ? (
          <>
            <span className="text-muted-foreground" data-testid="project-status">
              {project.status}
            </span>
            <span className="text-muted-foreground">{project.fileCount} files</span>
          </>
        ) : (
          <span className="text-muted-foreground">no project</span>
        )}
        {analysis.status === 'pending' ? (
<<<<<<< HEAD
          <span className="text-muted-foreground" data-testid="analysis-status">
            analyzing...
          </span>
        ) : null}
        {analysis.status === 'ready' ? (
          <span className="text-muted-foreground" data-testid="analysis-status">
            {analysis.outcome.chunksCreated} chunks
          </span>
        ) : null}
        {analysis.status === 'error' ? (
          <span className="text-destructive" data-testid="analysis-status" role="alert">
            {analysis.message}
=======
          <span className="flex items-center gap-1 text-muted-foreground">
            <Loader2 className="size-3 animate-spin" />
            analysing…
          </span>
        ) : analysis.status === 'ready' ? (
          <span className="text-muted-foreground">
            {analysis.outcome.chunksEmbedded} chunks · {analysis.outcome.filesParsed} parsed
          </span>
        ) : analysis.status === 'error' ? (
          <span className="text-destructive truncate" role="alert" title={analysis.message}>
            analysis failed
>>>>>>> 4c47d2aa1ccf6ef1885b16104e3665fca6828162
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
