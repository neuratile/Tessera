import type { ArtifactDetail, ArtifactSummary } from '@testing-ide/shared';
import { CheckCircle2, Download, Loader2, RefreshCw, X, XCircle } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { MarkdownView } from '@/components/markdown/markdown-view';
import { Button } from '@/components/ui/button';
import { exportMarkdownDocument } from '@/lib/export-markdown';
import { artifacts as artifactsIpc, generation, IpcError } from '@/lib/ipc';
import { useAiStore } from '@/stores/ai-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

type Props = {
  summary: ArtifactSummary;
  onClose: () => void;
};

/**
 * Slide-in detail view for one artifact: rendered markdown + lifecycle
 * actions + regenerate-with-feedback.
 *
 * Regenerate flow: posts a new `generate_artifact` request with
 * `parentId = summary.id` and the user's `reviewerFeedback`. The
 * Phase 5 `generation_service` chains the version (`max_version + 1`)
 * via `artifact_repo::insert`'s `parent_id` handling.
 */
export function ArtifactDetailDrawer({ summary, onClose }: Props) {
  const [detail, setDetail] = useState<ArtifactDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [feedback, setFeedback] = useState('');
  const [regenerating, setRegenerating] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportStatus, setExportStatus] = useState<string | null>(null);

  const project = useWorkspaceStore((s) => s.project);
  const activeProvider = useAiStore((s) => s.activeProvider);
  const upsertArtifact = useAiStore((s) => s.upsertArtifact);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const d = await artifactsIpc.getArtifact(summary.id);
        if (!cancelled) setDetail(d);
      } catch (err) {
        if (!cancelled) setError(err instanceof IpcError ? err.message : String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [summary.id]);

  const handleApprove = useCallback(() => {
    void (async () => {
      try {
        await artifactsIpc.approveArtifact(summary.id);
        upsertArtifact({ ...summary, status: 'approved' });
        if (detail !== null) setDetail({ ...detail, status: 'approved' });
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
      }
    })();
  }, [summary, detail, upsertArtifact]);

  const handleReject = useCallback(() => {
    void (async () => {
      try {
        await artifactsIpc.rejectArtifact(summary.id);
        upsertArtifact({ ...summary, status: 'rejected' });
        if (detail !== null) setDetail({ ...detail, status: 'rejected' });
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
      }
    })();
  }, [summary, detail, upsertArtifact]);

  const canRegenerate =
    project !== null &&
    activeProvider !== null &&
    typeof activeProvider.defaultModel === 'string' &&
    activeProvider.defaultModel.length > 0;

  const handleRegenerate = useCallback(() => {
    if (!canRegenerate || project === null || activeProvider === null) return;
    const model = activeProvider.defaultModel;
    if (typeof model !== 'string' || model.length === 0) return;
    setRegenerating(true);
    setError(null);
    void (async () => {
      try {
        const result = await generation.generateArtifact({
          projectId: project.id,
          projectName: project.name,
          artifactType: summary.artifactType,
          model,
          provider: activeProvider.provider,
          parentId: summary.id,
          reviewerFeedback: feedback,
        });
        const fresh = await artifactsIpc.getArtifact(result.artifactId);
        upsertArtifact({
          id: fresh.id,
          projectId: fresh.projectId,
          artifactType: fresh.artifactType,
          title: fresh.title,
          status: fresh.status,
          version: fresh.version,
          parentId: fresh.parentId ?? null,
          createdAt: fresh.createdAt,
          updatedAt: fresh.updatedAt,
          provider: fresh.provider,
          model: fresh.model,
        });
        // Replace the drawer's view with the fresh version so the user
        // sees the regenerated output immediately.
        setDetail(fresh);
        setFeedback('');
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
      } finally {
        setRegenerating(false);
      }
    })();
  }, [canRegenerate, project, activeProvider, summary, feedback, upsertArtifact]);

  const handleExportMarkdown = useCallback(() => {
    if (detail === null) {
      return;
    }

    setExporting(true);
    setError(null);
    setExportStatus(null);

    void (async () => {
      try {
        const exportedPath = await exportMarkdownDocument(detail.title, detail.contentMd);
        if (exportedPath !== null) {
          setExportStatus('Exported markdown.');
        }
      } catch (err) {
        setError(err instanceof IpcError ? err.message : String(err));
      } finally {
        setExporting(false);
      }
    })();
  }, [detail]);

  const isPending =
    detail?.status === 'draft' || detail?.status === 'in_review' || detail === null;

  return (
    <>
      <div
        className="bg-background/80 fixed inset-0 z-40 backdrop-blur-sm"
        onClick={onClose}
        aria-hidden="true"
      />
      <aside
        className="fixed inset-y-0 right-0 z-50 flex w-full max-w-2xl flex-col border-l border-border bg-background shadow-2xl"
        role="dialog"
        aria-label={`Review ${summary.title}`}
      >
        <header className="flex items-start justify-between gap-2 border-b border-border px-4 py-3">
          <div className="min-w-0">
            <p className="text-muted-foreground text-[10px] font-semibold uppercase tracking-wider">
              {summary.artifactType} · v{summary.version}
            </p>
            <h2 className="truncate text-base font-semibold tracking-tight" title={summary.title}>
              {summary.title}
            </h2>
            <p className="text-muted-foreground mt-0.5 text-[10px]">
              {summary.provider} · {summary.model}
            </p>
          </div>
          <Button type="button" size="icon" variant="ghost" onClick={onClose} aria-label="Close">
            <X className="size-4" />
          </Button>
        </header>

        <div className="flex-1 overflow-y-auto p-4">
          {error !== null ? (
            <p className="text-destructive text-sm" role="alert">
              {error}
            </p>
          ) : null}
          {loading ? (
            <p className="text-muted-foreground flex items-center gap-2 text-sm">
              <Loader2 className="size-3 animate-spin" /> Loading…
            </p>
          ) : detail !== null ? (
            <MarkdownView source={detail.contentMd} />
          ) : null}
        </div>

        <footer className="border-t border-border bg-card p-3 space-y-3">
          <div>
            <label
              htmlFor="reviewer-feedback"
              className="text-muted-foreground mb-1 block text-[10px] font-semibold uppercase tracking-wider"
            >
              Feedback for regeneration (optional)
            </label>
            <textarea
              id="reviewer-feedback"
              value={feedback}
              onChange={(e) => setFeedback(e.target.value)}
              placeholder="What should the next version do differently?"
              maxLength={4000}
              className="bg-background placeholder:text-muted-foreground/70 w-full resize-none rounded-md border border-border p-2 text-xs focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-1"
              rows={3}
            />
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {isPending ? (
              <>
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  onClick={handleApprove}
                  disabled={detail === null}
                >
                  <CheckCircle2 className="size-3.5" /> Approve
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={handleReject}
                  disabled={detail === null}
                >
                  <XCircle className="size-3.5" /> Reject
                </Button>
              </>
            ) : (
              <span
                className={`text-xs font-medium ${
                  detail?.status === 'approved'
                    ? 'text-green-600 dark:text-green-400'
                    : 'text-red-600 dark:text-red-400'
                }`}
              >
                {detail?.status === 'approved' ? 'Approved' : 'Rejected'}
              </span>
            )}
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={handleExportMarkdown}
              disabled={detail === null || exporting}
            >
              {exporting ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
              Export markdown
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={handleRegenerate}
              disabled={!canRegenerate || regenerating || detail === null}
              className="ml-auto"
            >
              {regenerating ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <RefreshCw className="size-3.5" />
              )}
              Regenerate
            </Button>
          </div>
          {!canRegenerate ? (
            <p className="text-muted-foreground text-[10px]">
              Configure a provider in Settings to enable regeneration.
            </p>
          ) : null}
          {exportStatus !== null ? <p className="text-muted-foreground text-[10px]">{exportStatus}</p> : null}
        </footer>
      </aside>
    </>
  );
}
