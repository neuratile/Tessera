import { RefreshCw, TriangleAlert } from 'lucide-react';
import { useEffect } from 'react';

import { Button } from '@/components/ui/button';
import { COMMAND, dispatchCommand } from '@/lib/command-bus';
import { useEmbeddingStore } from '@/stores/embedding-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Non-blocking stale-index banner (plan/EMBEDDING_PROVIDER_SELECT.md
 * §7.2). Shown between the toolbar and the workspace when the open
 * project's chunk index was built with a different embedding
 * provider/model than the active config — RAG retrieval is degraded
 * until the user re-indexes. Re-indexing is deliberately manual: the
 * button fires the same Analyze command as the toolbar.
 */
export function IndexStaleBanner() {
  const project = useWorkspaceStore((s) => s.project);
  const analysisStatus = useWorkspaceStore((s) => s.analysis.status);
  const indexStatus = useEmbeddingStore((s) => s.indexStatus);
  const refreshIndexStatus = useEmbeddingStore((s) => s.refreshIndexStatus);
  const clearIndexStatus = useEmbeddingStore((s) => s.clearIndexStatus);

  // Refresh on project open and whenever an analyze run settles —
  // a completed re-index is what clears the banner.
  useEffect(() => {
    if (project === null) {
      clearIndexStatus();
      return;
    }
    if (analysisStatus === 'pending') return;
    void refreshIndexStatus(project.id);
  }, [project, analysisStatus, refreshIndexStatus, clearIndexStatus]);

  if (
    project === null ||
    indexStatus === null ||
    !indexStatus.isStale ||
    indexStatus.projectId !== project.id ||
    analysisStatus === 'pending'
  ) {
    return null;
  }

  const indexedWith = indexStatus.indexedWith;

  return (
    <div
      className="border-warning/40 bg-warning/10 flex items-center gap-3 border-b px-3 py-1.5"
      role="status"
      data-testid="index-stale-banner"
    >
      <TriangleAlert className="text-warning size-4 shrink-0" aria-hidden="true" />
      <p className="text-foreground min-w-0 flex-1 truncate text-xs">
        Code index was built with{' '}
        <span className="font-mono">{indexedWith?.model ?? 'a previous model'}</span>; the
        active embedding model is{' '}
        <span className="font-mono">{indexStatus.activeConfig.model}</span>. Retrieval is
        degraded until you re-index.
      </p>
      <Button
        type="button"
        size="sm"
        variant="outline"
        className="h-6 shrink-0 px-2 text-[10px] font-semibold"
        onClick={() => dispatchCommand(COMMAND.AiAnalyze)}
        data-testid="index-stale-reindex"
      >
        <RefreshCw className="size-3" />
        Re-index now
      </Button>
    </div>
  );
}
