import type { ArtifactSummary, GenerationArtifactType } from '@testing-ide/shared';
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
} from '@/lib/ipc';
import { extractStreamingPreview } from '@/lib/partial-json';
import { pickActiveProvider } from '@/lib/provider';
import { useAiStore } from '@/stores/ai-store';
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
  const setActiveProvider = useAiStore((s) => s.setActiveProvider);
  const setProviders = useAiStore((s) => s.setProviders);
  const setGeneration = useAiStore((s) => s.setGeneration);
  const setArtifacts = useAiStore((s) => s.setArtifacts);
  const upsertArtifact = useAiStore((s) => s.upsertArtifact);
  const setLoadingArtifacts = useAiStore((s) => s.setLoadingArtifacts);
  const setArtifactsError = useAiStore((s) => s.setArtifactsError);
  const appendPartial = useAiStore((s) => s.appendPartial);

  const [openArtifact, setOpenArtifact] = useState<ArtifactSummary | null>(null);
  const [queueFilter, setQueueFilter] = useState('');

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
      return;
    }
    setLoadingArtifacts(true);
    void (async () => {
      try {
        const list = await artifactsIpc.listArtifacts(project.id);
        setArtifacts(list);
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
    (artifactType: GenerationArtifactType) => {
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
  // nothing to regenerate.
  useCommand(
    COMMAND.AiRegenerate,
    useCallback(() => {
      const last = lastGeneratedTypeRef.current;
      if (last === null) return;
      if (!canGenerate) return;
      handleGenerate(last);
    }, [canGenerate, handleGenerate]),
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

  console.log("DEBUG: AiPanel rendering, project is:", project);
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
            <p className="text-xs text-muted-foreground">
              None configured. Open Settings to add a provider.
            </p>
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

function ArtifactRow({
  artifact,
  onApprove,
  onReject,
  onOpen,
}: {
  artifact: ArtifactSummary;
  onApprove: (a: ArtifactSummary) => void;
  onReject: (a: ArtifactSummary) => void;
  onOpen: (a: ArtifactSummary) => void;
}) {
  const isPending = artifact.status === 'draft' || artifact.status === 'in_review';
  // Stitch artifact-card: 1px-wide status-colored stripe pinned to the
  // left edge so the queue can be scanned by colour at a glance.
  const stripe: Record<ArtifactSummary['status'], string> = {
    draft: 'bg-surface-3',
    in_review: 'bg-secondary',
    approved: 'bg-primary',
    rejected: 'bg-destructive',
  };
  return (
    <li className="group relative overflow-hidden rounded-md border border-border bg-card p-2 text-xs transition-colors hover:border-primary/50">
      <span
        aria-hidden="true"
        className={`absolute left-0 top-0 h-full w-1 ${stripe[artifact.status]}`}
      />
      <button
        type="button"
        onClick={() => onOpen(artifact)}
        className="-m-2 mb-0 block w-[calc(100%+1rem)] rounded-t-md p-2 pl-3 text-left transition-colors hover:bg-muted/40"
        aria-label={`Open ${artifact.title}`}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1">
            <p className="truncate font-mono text-foreground" title={artifact.title}>
              {artifact.title}
            </p>
            <p className="text-muted-foreground mt-0.5 text-[10px]">
              {artifact.artifactType} · v{artifact.version} ·{' '}
              <span className="font-mono">{artifact.model}</span>
            </p>
          </div>
          <StatusBadge status={artifact.status} />
        </div>
      </button>
      {isPending ? (
        <div className="mt-2 flex items-center gap-1 pl-1">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={() => onApprove(artifact)}
            className="flex-1"
          >
            <CheckCircle2 className="size-3" />
            Approve
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => onReject(artifact)}
            className="flex-1"
          >
            <XCircle className="size-3" />
            Reject
          </Button>
        </div>
      ) : null}
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
