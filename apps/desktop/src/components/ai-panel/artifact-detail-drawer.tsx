import type {
  ArtifactDetail,
  ArtifactSummary,
  ArtifactVersionSummary,
} from '@testing-ide/shared';
import { CheckCircle2, Download, GitCompare, Loader2, RefreshCw, X, XCircle } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { MarkdownView } from '@/components/markdown/markdown-view';
import { Button } from '@/components/ui/button';
import { Dialog } from '@/components/ui/dialog';
import { toArtifactSummary } from '@/lib/artifact';
import { useDialogTitleId } from '@/lib/dialog-title';
import { exportMarkdownDocument } from '@/lib/export-markdown';
import { artifacts as artifactsIpc, generation, getErrorMessage } from '@/lib/ipc';
import { useAiStore } from '@/stores/ai-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { DiffView } from '@/components/ai-panel/diff-view';

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

  // Version chain — fetched lazily when the version picker or diff
  // toggle is used. Empty array means "not loaded yet"; once loaded
  // includes at least one entry (the current artifact itself).
  const [chain, setChain] = useState<ArtifactVersionSummary[]>([]);
  const [chainLoading, setChainLoading] = useState(false);

  // Drawer view mode + comparison base. Diff mode renders a
  // line-level unified diff against the body of `compareId`; the
  // default base is the artifact's `parentId` so "show diff" is
  // sensible on a regenerated v2/v3/… without any picker work.
  const [viewMode, setViewMode] = useState<'content' | 'diff'>('content');
  const [compareId, setCompareId] = useState<string | null>(null);
  const [compareDetail, setCompareDetail] = useState<ArtifactDetail | null>(null);
  const [compareLoading, setCompareLoading] = useState(false);

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
        if (!cancelled) setError(getErrorMessage(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [summary.id]);

  // Fetch the full version chain when the drawer mounts so the
  // "Diff" toggle in the header can flip to diff mode in a single
  // click. The chain endpoint is cheap (lightweight projection) so
  // the eager fetch is fine even for artifacts the user never
  // diffs.
  useEffect(() => {
    let cancelled = false;
    setChainLoading(true);
    void (async () => {
      try {
        const list = await artifactsIpc.listArtifactVersions(summary.id);
        if (cancelled) return;
        setChain(list);
        // Default comparison target = direct parent. Fall back to
        // "previous version in the chain" when the current artifact
        // has no parent_id but the chain has earlier entries (the
        // user may have opened a child whose root is older).
        const fallback =
          summary.parentId ??
          (() => {
            const idx = list.findIndex((v) => v.id === summary.id);
            return idx > 0 ? (list[idx - 1]?.id ?? null) : null;
          })();
        setCompareId(fallback);
      } catch {
        // Chain is a UI nicety — if it fails the user can still
        // approve / reject / regenerate, so swallow the error.
      } finally {
        if (!cancelled) setChainLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [summary.id, summary.parentId]);

  // Lazy-load the comparison body once the user actually flips to
  // diff mode AND a base is selected. Caches per-id so flipping
  // back and forth between content + diff does not re-fetch.
  useEffect(() => {
    if (viewMode !== 'diff') return;
    if (compareId === null) return;
    if (compareDetail !== null && compareDetail.id === compareId) return;
    let cancelled = false;
    setCompareLoading(true);
    void (async () => {
      try {
        const d = await artifactsIpc.getArtifact(compareId);
        if (!cancelled) setCompareDetail(d);
      } catch (err) {
        if (!cancelled) setError(getErrorMessage(err));
      } finally {
        if (!cancelled) setCompareLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [viewMode, compareId, compareDetail]);

  const baseVersion = useMemo(() => {
    if (compareId === null) return null;
    return chain.find((v) => v.id === compareId) ?? null;
  }, [chain, compareId]);

  const canDiff = chain.length > 1 && compareId !== null && compareId !== summary.id;

  const handleApprove = useCallback(() => {
    void (async () => {
      try {
        await artifactsIpc.approveArtifact(summary.id);
        upsertArtifact({ ...summary, status: 'approved' });
        if (detail !== null) setDetail({ ...detail, status: 'approved' });
      } catch (err) {
        setError(getErrorMessage(err));
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
        setError(getErrorMessage(err));
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
        upsertArtifact(toArtifactSummary(fresh));
        // Replace the drawer's view with the fresh version so the user
        // sees the regenerated output immediately.
        setDetail(fresh);
        setFeedback('');
      } catch (err) {
        setError(getErrorMessage(err));
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
        setError(getErrorMessage(err));
      } finally {
        setExporting(false);
      }
    })();
  }, [detail]);

  const isPending =
    detail?.status === 'draft' || detail?.status === 'in_review' || detail === null;
  const titleId = useDialogTitleId();

  return (
    <Dialog open onClose={onClose} labelledBy={titleId} widthClass="max-w-2xl">
      <header className="flex items-start justify-between gap-2 border-b border-border bg-card px-4 py-3">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <p className="text-muted-foreground text-[10px] font-semibold uppercase tracking-[0.12em]">
                {summary.artifactType} · v{summary.version}
              </p>
              <span className={`pill pill-${summary.status.replace('_', '-')}`}>
                {summary.status.replace('_', ' ')}
              </span>
              {chain.length > 1 ? (
                <select
                  aria-label="Compare against version"
                  value={compareId ?? ''}
                  onChange={(e) => {
                    const next = e.target.value.length === 0 ? null : e.target.value;
                    setCompareId(next);
                    setCompareDetail(null);
                  }}
                  className="bg-surface-2 border-border focus-visible:border-primary focus-visible:ring-primary/40 text-muted-foreground hover:text-foreground h-5 rounded border px-1 font-mono text-[10px] transition-colors focus-visible:outline-none focus-visible:ring-2"
                >
                  <option value="">no compare</option>
                  {chain
                    .filter((v) => v.id !== summary.id)
                    .map((v) => (
                      <option key={v.id} value={v.id}>
                        compare vs v{v.version}
                      </option>
                    ))}
                </select>
              ) : null}
            </div>
            <h2
              id={titleId}
              className="mt-1 truncate font-mono text-sm font-semibold text-foreground"
              title={summary.title}
            >
              {summary.title}
            </h2>
            <p className="text-muted-foreground mt-0.5 font-mono text-[10px]">
              {summary.provider} · {summary.model}
            </p>
          </div>
          <div className="flex items-center gap-1">
            <Button
              type="button"
              size="sm"
              variant={viewMode === 'diff' ? 'secondary' : 'ghost'}
              onClick={() => setViewMode((m) => (m === 'diff' ? 'content' : 'diff'))}
              disabled={!canDiff}
              aria-pressed={viewMode === 'diff'}
              aria-label="Toggle diff view"
              title={canDiff ? 'Toggle diff view' : 'Need at least two versions to diff'}
            >
              <GitCompare className="size-3.5" />
              Diff
            </Button>
            <Button type="button" size="icon" variant="ghost" onClick={onClose} aria-label="Close">
              <X className="size-4" />
            </Button>
          </div>
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
          ) : viewMode === 'diff' ? (
            <DiffBody
              chainLoading={chainLoading}
              compareLoading={compareLoading}
              currentDetail={detail}
              compareDetail={compareDetail}
              currentVersion={summary.version}
              baseVersion={baseVersion?.version ?? null}
            />
          ) : detail !== null ? (
            <MarkdownView source={detail.contentMd} />
          ) : null}
        </div>

        <footer className="border-t border-border bg-surface-3 p-3 space-y-3">
          <div>
            <label
              htmlFor="reviewer-feedback"
              className="text-muted-foreground mb-1 block text-[10px] font-semibold uppercase tracking-[0.12em]"
            >
              Feedback for regeneration (optional)
            </label>
            <textarea
              id="reviewer-feedback"
              value={feedback}
              onChange={(e) => setFeedback(e.target.value)}
              placeholder="What should the next version do differently?"
              maxLength={4000}
              className="bg-background placeholder:text-muted-foreground/70 focus-visible:border-primary focus-visible:ring-primary/20 w-full resize-none rounded-md border border-border p-2 text-xs transition-colors focus-visible:outline-none focus-visible:ring-2"
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
                className={`pill ${
                  detail?.status === 'approved' ? 'pill-approved' : 'pill-rejected'
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
    </Dialog>
  );
}

/**
 * Diff-mode body. Pulled out of the main component so the
 * loading-state ternary in the dialog stays readable.
 */
function DiffBody({
  chainLoading,
  compareLoading,
  currentDetail,
  compareDetail,
  currentVersion,
  baseVersion,
}: {
  chainLoading: boolean;
  compareLoading: boolean;
  currentDetail: ArtifactDetail | null;
  compareDetail: ArtifactDetail | null;
  currentVersion: number;
  baseVersion: number | null;
}) {
  if (chainLoading || compareLoading) {
    return (
      <p className="text-muted-foreground flex items-center gap-2 text-sm">
        <Loader2 className="size-3 animate-spin" />
        Loading versions…
      </p>
    );
  }
  if (currentDetail === null) {
    return null;
  }
  if (compareDetail === null || baseVersion === null) {
    return (
      <p className="text-muted-foreground text-xs">
        No earlier version to compare against. Pick a base in the header dropdown.
      </p>
    );
  }
  return (
    <DiffView
      before={compareDetail.contentMd}
      after={currentDetail.contentMd}
      beforeLabel={`v${baseVersion}`}
      afterLabel={`v${currentVersion}`}
    />
  );
}
