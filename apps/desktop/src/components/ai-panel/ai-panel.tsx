import type { ArtifactSummary, GenerationArtifactType, ExternalLink } from '@testing-ide/shared';
import { toast } from '@/stores/toast-store';
import {
  Bug,
  CheckCircle2,
  ClipboardList,
  FileBarChart,
  FileText,
  Loader2,
  RefreshCw,
  Search,
  XCircle,
  ArrowUpRight,
  X,
} from 'lucide-react';
import type { ReactNode } from 'react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { toArtifactSummary } from '@/lib/artifact';
import { COMMAND, useCommand } from '@/lib/command-bus';
import {
  artifacts as artifactsIpc,
  generation,
  getErrorMessage,
  providers,
  streaming,
  trackers,
} from '@/lib/ipc';
import { extractStreamingPreview } from '@/lib/partial-json';
import { pickActiveProvider } from '@/lib/provider';
import { useAiStore } from '@/stores/ai-store';
import { useUiStore } from '@/stores/ui-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { ArtifactDetailDrawer } from '@/components/ai-panel/artifact-detail-drawer';

const GENERATE_BUTTONS: ReadonlyArray<{
  id: GenerationArtifactType;
  label: string;
  icon: ReactNode;
}> = [
  { id: 'context-md', label: 'Context', icon: <FileText className="size-3.5" /> },
  { id: 'test-plan', label: 'Test plan', icon: <ClipboardList className="size-3.5" /> },
  { id: 'test-cases', label: 'Test cases', icon: <CheckCircle2 className="size-3.5" /> },
  { id: 'defect-report', label: 'Defects', icon: <Bug className="size-3.5" /> },
  { id: 'bug-report', label: 'Bugs', icon: <FileBarChart className="size-3.5" /> },
];

/**
 * Right-sidebar AI action panel.
 *
 * Backend wiring:
 * - `list_artifacts` populates the review queue on project change.
 * - `generate_artifact` triggers Phase 5 generation against the active
 *   provider config; the resulting artifact is fetched + prepended to
 *   the queue.
 * - `approve_artifact` / `reject_artifact` flip lifecycle status.
 *
 * Streaming preview is deferred — see comment in `ai-store.ts`.
 */
export function AiPanel() {
  const project = useWorkspaceStore((s) => s.project);
  const generationStatus = useAiStore((s) => s.generation);
  const reviewQueue = useAiStore((s) => s.artifacts);
  const loadingArtifacts = useAiStore((s) => s.loadingArtifacts);
  const artifactsError = useAiStore((s) => s.artifactsError);
  const activeProvider = useAiStore((s) => s.activeProvider);
  const providerList = useAiStore((s) => s.providers);
  const setActiveProvider = useAiStore((s) => s.setActiveProvider);
  const setProviders = useAiStore((s) => s.setProviders);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const setGeneration = useAiStore((s) => s.setGeneration);
  const setArtifacts = useAiStore((s) => s.setArtifacts);
  const upsertArtifact = useAiStore((s) => s.upsertArtifact);
  const setLoadingArtifacts = useAiStore((s) => s.setLoadingArtifacts);
  const setArtifactsError = useAiStore((s) => s.setArtifactsError);
  const appendPartial = useAiStore((s) => s.appendPartial);

  const [providersLoaded, setProvidersLoaded] = useState(false);
  const [openArtifact, setOpenArtifact] = useState<ArtifactSummary | null>(null);
  const [queueFilter, setQueueFilter] = useState('');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [externalLinks, setExternalLinks] = useState<Record<string, ExternalLink[]>>({});
  const [bulkPushing, setBulkPushing] = useState(false);

  // Case-insensitive substring match across title + artifact-type +
  // model. Empty query → full queue. Memoised so we don't re-scan on
  // every render of the streaming preview / partial buffer.
  const filteredQueue = useMemo(() => {
    const needle = queueFilter.trim().toLowerCase();
    if (needle.length === 0) return reviewQueue;
    return reviewQueue.filter((a) => {
      const hay = `${a.title} ${a.artifactType} ${a.model}`.toLowerCase();
      return hay.includes(needle);
    });
  }, [reviewQueue, queueFilter]);

  // Subscribe to streaming events on mount. Backend emits on every
  // `generate_artifact` invocation; the listener filters via the
  // `generationId` field so concurrent generations do not cross-wire.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        unlisten = await streaming.subscribeToGenerationEvents((event) => {
          if (cancelled) return;
          if (event.kind === 'tool_args' || event.kind === 'text') {
            if (typeof event.delta === 'string') {
              appendPartial(event.delta);
            }
          }
        });
      } catch {
        // Streaming events are best-effort. The await on
        // `generate_artifact` still produces the final outcome.
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten !== null) unlisten();
    };
  }, [appendPartial]);

  // Pull the active provider config the first time the panel renders
  // and after a project loads. The generation panel cannot run without
  // one, so we surface a clear message when none is configured. The
  // full list is also cached on the store so the status-bar provider
  // switcher can render it without re-fetching on every render.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await providers.listProviderConfigs();
        if (cancelled) return;
        setProviders(list);
        setActiveProvider(pickActiveProvider(list));
      } catch {
        if (!cancelled) {
          setProviders([]);
          setActiveProvider(null);
        }
      } finally {
        if (!cancelled) setProvidersLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [setActiveProvider, setProviders]);

  // Refresh the review queue whenever the project changes.
  const refreshArtifacts = useCallback(() => {
    if (project === null) {
      setArtifacts([]);
      setExternalLinks({});
      return;
    }
    setLoadingArtifacts(true);
    void (async () => {
      try {
        const [list, linksList] = await Promise.all([
          artifactsIpc.listArtifacts(project.id),
          trackers.listExternalLinks(),
        ]);
        setArtifacts(list);
        setExternalLinks(groupLinksByArtifact(linksList));
      } catch (err) {
        setArtifactsError(getErrorMessage(err));
      } finally {
        setLoadingArtifacts(false);
      }
    })();
  }, [project, setArtifacts, setArtifactsError, setLoadingArtifacts]);

  useEffect(() => {
    refreshArtifacts();
  }, [refreshArtifacts]);

  const canGenerate = useMemo(
    () =>
      project !== null &&
      activeProvider !== null &&
      typeof activeProvider.defaultModel === 'string' &&
      activeProvider.defaultModel.length > 0 &&
      generationStatus.status !== 'pending',
    [project, activeProvider, generationStatus.status],
  );

  // Track the last artifact type the user generated so the "Regenerate
  // last" menu item / `Cmd/Ctrl+G` shortcut has something to re-fire.
  // Kept in a ref so the AiRegenerate listener does not re-bind on
  // every generation cycle.
  const lastGeneratedTypeRef = useRef<GenerationArtifactType | null>(null);

  const handleGenerate = useCallback(
    (artifactType: GenerationArtifactType, parentId?: string) => {
      if (project === null || activeProvider === null) return;
      const model = activeProvider.defaultModel;
      if (typeof model !== 'string' || model.length === 0) return;
      lastGeneratedTypeRef.current = artifactType;
      setGeneration({ status: 'pending', artifactType, partial: '' });
      void (async () => {
        try {
          const result = await generation.generateArtifact({
            projectId: project.id,
            projectName: project.name,
            artifactType,
            model,
            provider: activeProvider.provider,
            ...(parentId !== undefined ? { parentId } : {}),
          });
          // Pull the freshly-saved row so we have the canonical metadata
          // (status, version, parent chain) rather than reconstructing
          // a partial summary in JS.
          const detail = await artifactsIpc.getArtifact(result.artifactId);
          upsertArtifact(toArtifactSummary(detail));
          setGeneration({ status: 'idle' });
        } catch (err) {
          setGeneration({
            status: 'error',
            message: getErrorMessage(err),
          });
        }
      })();
    },
    [project, activeProvider, setGeneration, upsertArtifact],
  );

  // Command palette generation bridge — palette dispatches a window
  // event with the desired artifact type rather than fanning through
  // the bus (the bus is per-id and we'd need five literals). One
  // listener routes any of the five generator types into the same
  // `handleGenerate` the on-screen buttons invoke.
  useEffect(() => {
    const known: ReadonlyArray<GenerationArtifactType> = [
      'context-md',
      'test-plan',
      'test-cases',
      'defect-report',
      'bug-report',
    ];
    const isGenerationArtifactType = (v: unknown): v is GenerationArtifactType =>
      typeof v === 'string' && (known as ReadonlyArray<string>).includes(v);
    const handler = (event: Event) => {
      if (!(event instanceof CustomEvent)) return;
      const detail: unknown = event.detail;
      if (isGenerationArtifactType(detail)) {
        handleGenerate(detail);
      }
    };
    window.addEventListener('palette:generate', handler);
    return () => window.removeEventListener('palette:generate', handler);
  }, [handleGenerate]);

  // Regenerate-last command: re-runs `handleGenerate` against the
  // most recent artifact type chosen via the button grid. Silently
  // no-ops on the very first session before any generator has been
  // clicked — the menu item still shows, but `Cmd/Ctrl+G` is a
  // no-op rather than an error, which feels right when there is
  // nothing to regenerate. Chains the new artifact onto the newest
  // existing artifact of that type (queue is newest-first) so a menu /
  // shortcut regenerate bumps the version instead of creating an
  // orphan v1.
  useCommand(
    COMMAND.AiRegenerate,
    useCallback(() => {
      const last = lastGeneratedTypeRef.current;
      if (last === null) return;
      if (!canGenerate) return;
      const parent = reviewQueue.find((a) => a.artifactType === last);
      handleGenerate(last, parent?.id);
    }, [canGenerate, handleGenerate, reviewQueue]),
  );

  const handleApprove = useCallback(
    (artifact: ArtifactSummary) => {
      void (async () => {
        try {
          await artifactsIpc.approveArtifact(artifact.id);
          upsertArtifact({ ...artifact, status: 'approved' });
        } catch (err) {
          setArtifactsError(getErrorMessage(err));
        }
      })();
    },
    [setArtifactsError, upsertArtifact],
  );

  const handleReject = useCallback(
    (artifact: ArtifactSummary) => {
      void (async () => {
        try {
          await artifactsIpc.rejectArtifact(artifact.id);
          upsertArtifact({ ...artifact, status: 'rejected' });
        } catch (err) {
          setArtifactsError(getErrorMessage(err));
        }
      })();
    },
    [setArtifactsError, upsertArtifact],
  );

  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
  }, []);

  const handleBulkPush = useCallback(() => {
    const ids = Array.from(selectedIds);
    if (ids.length === 0) return;
    setBulkPushing(true);
    void (async () => {
      try {
        const results = await trackers.bulkPushArtifactsToJira(ids);
        
        const succeeded = results.filter((r) => r.success);
        const failed = results.filter((r) => !r.success);
        
        if (succeeded.length > 0) {
          toast.ok(`Pushed ${succeeded.length} artifact(s) to Jira.`, { title: 'Bulk Push' });
        }
        if (failed.length > 0) {
          toast.err(`Failed to push ${failed.length} artifact(s).`, { title: 'Bulk Push' });
        }
        
        const linksList = await trackers.listExternalLinks();
        setExternalLinks(groupLinksByArtifact(linksList));
        clearSelection();
      } catch (err) {
        toast.err(`Bulk push failed: ${getErrorMessage(err)}`, { title: 'Bulk Push' });
      } finally {
        setBulkPushing(false);
      }
    })();
  }, [selectedIds, clearSelection]);

  const handleBulkApprove = useCallback(() => {
    const ids = Array.from(selectedIds);
    if (ids.length === 0) return;
    void (async () => {
      try {
        await Promise.all(ids.map((id) => artifactsIpc.approveArtifact(id)));
        
        for (const id of ids) {
          const artifact = reviewQueue.find((a) => a.id === id);
          if (artifact) {
            upsertArtifact({ ...artifact, status: 'approved' });
          }
        }
        toast.ok(`Approved ${ids.length} artifact(s).`, { title: 'Bulk Approval' });
        clearSelection();
      } catch (err) {
        toast.err(`Bulk approval failed: ${getErrorMessage(err)}`, { title: 'Bulk Approval' });
      }
    })();
  }, [selectedIds, reviewQueue, upsertArtifact, clearSelection]);

  const handleRefreshLinkStatus = useCallback(async (linkId: string) => {
    try {
      const updated = await trackers.refreshExternalLinkStatus(linkId);
      setExternalLinks((prev) => {
        const current = prev[updated.artifactId] ?? [];
        const next = current.some((l) => l.id === updated.id)
          ? current.map((l) => (l.id === updated.id ? updated : l))
          : [...current, updated];
        return { ...prev, [updated.artifactId]: next };
      });
      toast.ok(`Refreshed Jira status to: ${updated.lastStatus || 'Unknown'}`, { title: 'Jira Integration' });
    } catch (err) {
      toast.err(`Failed to refresh status: ${getErrorMessage(err)}`, { title: 'Jira Integration' });
    }
  }, []);

  const hasSelection = selectedIds.size > 0;

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-border px-3 h-8 flex items-center justify-between bg-card">
        <h2 className="text-[11px] font-semibold uppercase tracking-[0.12em] text-foreground">
          AI Inspection
        </h2>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Refresh review queue"
          onClick={refreshArtifacts}
          disabled={project === null || loadingArtifacts}
        >
          <RefreshCw className={`size-4 ${loadingArtifacts ? 'animate-spin' : ''}`} />
        </Button>
      </div>

      <div className="border-b border-border p-3 space-y-4">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground mb-1">
            Provider
          </p>
          {activeProvider === null ? (
            !providersLoaded ? (
              <p className="text-xs text-muted-foreground">Loading connections…</p>
            ) : providerList.length === 0 ? (
              <p className="text-xs text-muted-foreground">
                None configured. Open Settings to add a provider.
              </p>
            ) : (
              <div className="space-y-1.5">
                <p className="text-xs text-muted-foreground" role="alert">
                  No connection selected. Pick one to generate.
                </p>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setSettingsOpen(true)}
                  className="h-7 text-[11px]"
                >
                  Select a connection
                </Button>
              </div>
            )
          ) : (
            <p className="text-xs">
              <span className="font-medium text-foreground">{activeProvider.provider}</span>
              {typeof activeProvider.defaultModel === 'string' &&
              activeProvider.defaultModel.length > 0 ? (
                <span className="text-muted-foreground font-mono">
                  {' · '}
                  {activeProvider.defaultModel}
                </span>
              ) : null}
            </p>
          )}
        </div>

        <div>
          <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground mb-2">
            Generate Artifacts
          </p>
          <div className="grid grid-cols-2 gap-1.5">
            {GENERATE_BUTTONS.map((b) => (
              <Button
                key={b.id}
                type="button"
                variant="outline"
                size="sm"
                onClick={() => handleGenerate(b.id)}
                disabled={!canGenerate}
                className="justify-start gap-2"
              >
                {b.icon}
                {b.label}
              </Button>
            ))}
          </div>
        </div>

        {generationStatus.status === 'pending' ? (
          <div className="space-y-1.5">
            <p className="text-muted-foreground flex items-center gap-2 text-xs">
              <Loader2 className="size-3 animate-spin" />
              Generating {generationStatus.artifactType}…
            </p>
            {generationStatus.partial.length > 0 ? (
              <StreamingPreview buffer={generationStatus.partial} />
            ) : null}
          </div>
        ) : generationStatus.status === 'error' ? (
          <p className="text-destructive text-xs" role="alert">
            {generationStatus.message}
          </p>
        ) : null}
      </div>

      <div className="flex-1 overflow-y-auto p-3">
        <div className="mb-2 flex items-center justify-between">
          <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
            Review Queue
          </p>
          <span className="rounded-sm bg-surface-3 px-1.5 py-0.5 text-[10px] text-muted-foreground">
            {filteredQueue.length}
            {queueFilter.length > 0 ? ` / ${reviewQueue.length}` : ''}{' '}
            {reviewQueue.length === 1 ? 'item' : 'items'}
          </span>
        </div>

        {hasSelection ? (
          <div className="mb-3 flex items-center justify-between gap-2 bg-muted/60 border border-border p-2 rounded-md">
            <span className="text-xs font-semibold text-foreground font-mono">
              {selectedIds.size} selected
            </span>
            <div className="flex items-center gap-1.5">
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={handleBulkPush}
                disabled={bulkPushing}
                className="h-7 text-[10px] px-2 font-mono"
              >
                {bulkPushing ? (
                  <Loader2 className="size-3 animate-spin mr-1 text-primary" />
                ) : null}
                Push to Jira
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={handleBulkApprove}
                className="h-7 text-[10px] px-2 font-mono"
              >
                Approve
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={clearSelection}
                className="h-7 w-7 p-0 flex items-center justify-center"
                title="Clear selection"
              >
                <X className="size-3.5" />
              </Button>
            </div>
          </div>
        ) : null}

        {reviewQueue.length > 0 ? (
          <div className="relative mb-2">
            <Search className="text-muted-foreground absolute left-2 top-1/2 size-3.5 -translate-y-1/2" />
            <input
              type="search"
              value={queueFilter}
              onChange={(e) => setQueueFilter(e.target.value)}
              placeholder="Filter artifacts…"
              aria-label="Filter review queue"
              className="bg-background border-border placeholder:text-muted-foreground focus-visible:border-primary focus-visible:ring-primary/40 h-7 w-full rounded-md border pl-7 pr-2 font-mono text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-2"
            />
          </div>
        ) : null}

        {artifactsError !== null ? (
          <p className="text-destructive text-xs" role="alert">
            {artifactsError}
          </p>
        ) : null}
        {project === null ? (
          <p className="text-muted-foreground text-xs">Open a project to see artifacts.</p>
        ) : reviewQueue.length === 0 ? (
          <p className="text-muted-foreground text-xs">
            No artifacts yet. Pick a generator above.
          </p>
        ) : filteredQueue.length === 0 ? (
          <p className="text-muted-foreground text-xs">
            No artifacts match <code className="font-mono">{queueFilter}</code>.
          </p>
        ) : (
          <ul className="space-y-2">
            {filteredQueue.map((a) => (
              <ArtifactRow
                key={a.id}
                artifact={a}
                onApprove={handleApprove}
                onReject={handleReject}
                onOpen={setOpenArtifact}
                isSelected={selectedIds.has(a.id)}
                onToggleSelect={toggleSelect}
                links={externalLinks[a.id] ?? []}
                onRefreshLinkStatus={handleRefreshLinkStatus}
              />
            ))}
          </ul>
        )}
      </div>
      {openArtifact !== null ? (
        <ArtifactDetailDrawer
          summary={openArtifact}
          onClose={() => setOpenArtifact(null)}
        />
      ) : null}
    </div>
  );
}

/**
 * Group external links by their artifact id. One artifact can map to many
 * issues (a TestCases artifact pushes one Jira issue per case), so the value
 * is a list — never collapse it to a single link or all but one is lost.
 */
function groupLinksByArtifact(links: ExternalLink[]): Record<string, ExternalLink[]> {
  const map: Record<string, ExternalLink[]> = {};
  for (const link of links) {
    (map[link.artifactId] ??= []).push(link);
  }
  return map;
}

function ArtifactRow({
  artifact,
  onApprove,
  onReject,
  onOpen,
  isSelected,
  onToggleSelect,
  links,
  onRefreshLinkStatus,
}: {
  artifact: ArtifactSummary;
  onApprove: (a: ArtifactSummary) => void;
  onReject: (a: ArtifactSummary) => void;
  onOpen: (a: ArtifactSummary) => void;
  isSelected: boolean;
  onToggleSelect: (id: string) => void;
  links: ExternalLink[];
  onRefreshLinkStatus: (linkId: string) => Promise<void>;
}) {
  const isPending = artifact.status === 'draft' || artifact.status === 'in_review';
  const [refreshingId, setRefreshingId] = useState<string | null>(null);

  // Stitch artifact-card: 1px-wide status-colored stripe pinned to the
  // left edge so the queue can be scanned by colour at a glance.
  const stripe: Record<ArtifactSummary['status'], string> = {
    draft: 'bg-surface-3',
    in_review: 'bg-secondary',
    approved: 'bg-primary',
    rejected: 'bg-destructive',
  };

  const handleRefresh = useCallback((e: React.MouseEvent, linkId: string) => {
    e.stopPropagation();
    setRefreshingId(linkId);
    void (async () => {
      try {
        await onRefreshLinkStatus(linkId);
      } finally {
        setRefreshingId(null);
      }
    })();
  }, [onRefreshLinkStatus]);

  return (
    <li className="group relative overflow-hidden rounded-md border border-border bg-card p-2 text-xs transition-colors hover:border-primary/50 flex items-start gap-2 pl-3">
      <span
        aria-hidden="true"
        className={`absolute left-0 top-0 h-full w-1 ${stripe[artifact.status]}`}
      />

      <input
        type="checkbox"
        checked={isSelected}
        onChange={() => onToggleSelect(artifact.id)}
        className="mt-1.5 size-3.5 accent-primary shrink-0 cursor-pointer"
        aria-label={`Select ${artifact.title}`}
      />

      <div className="min-w-0 flex-1 space-y-1">
        <button
          type="button"
          onClick={() => onOpen(artifact)}
          className="block w-full text-left font-mono text-foreground hover:text-primary transition-colors truncate font-semibold"
          aria-label={`Open ${artifact.title}`}
          title={artifact.title}
        >
          {artifact.title}
        </button>

        <div className="flex items-center justify-between gap-2 flex-wrap">
          <span className="text-muted-foreground text-[10px]">
            {artifact.artifactType} · v{artifact.version} ·{' '}
            <span className="font-mono">{artifact.model}</span>
          </span>
          <StatusBadge status={artifact.status} />
        </div>

        {links.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 mt-1">
            {links.map((link) => (
              <div
                key={link.id}
                className="flex items-center gap-1.5 bg-muted/50 border border-border/80 rounded px-1.5 py-0.5 text-[9px] w-fit font-mono"
              >
                <a
                  href={link.issueUrl}
                  target="_blank"
                  rel="noreferrer noopener"
                  className="text-primary hover:underline flex items-center gap-0.5 font-semibold"
                >
                  {link.issueKey}
                  {link.lastStatus ? ` (${link.lastStatus})` : ''}
                  <ArrowUpRight className="size-2.5" />
                </a>
                <button
                  type="button"
                  onClick={(e) => handleRefresh(e, link.id)}
                  disabled={refreshingId === link.id}
                  className="text-muted-foreground hover:text-foreground transition-colors p-0.5 rounded hover:bg-muted"
                  title="Refresh Jira status"
                >
                  <RefreshCw className={`size-2.5 ${refreshingId === link.id ? 'animate-spin' : ''}`} />
                </button>
              </div>
            ))}
          </div>
        )}

        {isPending && (
          <div className="mt-2 flex items-center gap-1 pl-1">
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() => onApprove(artifact)}
              className="flex-1 h-7 text-[10px]"
            >
              <CheckCircle2 className="size-3" />
              Approve
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => onReject(artifact)}
              className="flex-1 h-7 text-[10px]"
            >
              <XCircle className="size-3" />
              Reject
            </Button>
          </div>
        )}
      </div>
    </li>
  );
}

/**
 * Trim the streaming preview to the trailing N chars so the rendered
 * `<pre>` does not grow unbounded for long generations. Preserves the
 * tail because that is the most-recent (most-interesting) part of the
 * output stream.
 */
function trimPartialPreview(buffer: string): string {
  const MAX = 800;
  if (buffer.length <= MAX) return buffer;
  return `…${buffer.slice(-MAX)}`;
}

/**
 * Streaming preview block. Tries to extract a human-readable
 * "preview" field from the partial tool-call JSON (`summary`,
 * `description`, `title`, etc. — see
 * `lib/partial-json::PREVIEW_KEYS`) and renders it as prose with a
 * blinking caret. Falls back to the trailing raw-JSON tail when no
 * preview key has streamed in yet, so the user always sees motion
 * rather than a frozen "Generating…" line.
 */
function StreamingPreview({ buffer }: { buffer: string }) {
  const prose = useMemo(() => extractStreamingPreview(buffer), [buffer]);
  if (prose !== null && prose.length > 0) {
    return (
      <div className="bg-muted text-foreground max-h-32 overflow-y-auto rounded p-2 text-[11px] leading-snug">
        <p className="text-muted-foreground mb-1 text-[9px] font-semibold uppercase tracking-[0.1em]">
          Live preview
        </p>
        <span className="whitespace-pre-wrap break-words">{prose}</span>
        <span aria-hidden="true" className="bg-primary ml-0.5 inline-block h-3 w-1 animate-pulse" />
      </div>
    );
  }
  return (
    <pre className="bg-muted text-muted-foreground max-h-32 overflow-y-auto rounded p-2 font-mono text-[10px] leading-snug">
      {trimPartialPreview(buffer)}
    </pre>
  );
}

function StatusBadge({ status }: { status: ArtifactSummary['status'] }) {
  // Stitch DESIGN.md §Components — pill is fully rounded, uppercase,
  // status-colored from the design tokens. Backed by `.pill-*` classes
  // in `index.css` so the palette is the single source of truth.
  const klass: Record<ArtifactSummary['status'], string> = {
    draft: 'pill pill-draft',
    in_review: 'pill pill-in-review',
    approved: 'pill pill-approved',
    rejected: 'pill pill-rejected',
  };
  return (
    <span className={`${klass[status]} shrink-0`}>
      {status.replace('_', ' ')}
    </span>
  );
}
