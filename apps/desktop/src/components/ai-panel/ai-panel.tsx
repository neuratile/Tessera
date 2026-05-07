import type { ArtifactSummary, GenerationArtifactType } from '@testing-ide/shared';
import {
  Bug,
  CheckCircle2,
  ClipboardList,
  FileBarChart,
  FileText,
  Loader2,
  RefreshCw,
  XCircle,
} from 'lucide-react';
import type { ReactNode } from 'react';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  artifacts as artifactsIpc,
  generation,
  IpcError,
  providers,
  streaming,
} from '@/lib/ipc';
import { useAiStore } from '@/stores/ai-store';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { ArtifactDetailDrawer } from './artifact-detail-drawer';

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
  const setGeneration = useAiStore((s) => s.setGeneration);
  const setArtifacts = useAiStore((s) => s.setArtifacts);
  const upsertArtifact = useAiStore((s) => s.upsertArtifact);
  const setLoadingArtifacts = useAiStore((s) => s.setLoadingArtifacts);
  const setArtifactsError = useAiStore((s) => s.setArtifactsError);
  const appendPartial = useAiStore((s) => s.appendPartial);

  const [openArtifact, setOpenArtifact] = useState<ArtifactSummary | null>(null);

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
  // one, so we surface a clear message when none is configured.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await providers.listProviderConfigs();
        if (cancelled) return;
        const active = list.find((c) => c.isActive) ?? list[0] ?? null;
        setActiveProvider(active);
      } catch {
        if (!cancelled) setActiveProvider(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [setActiveProvider]);

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
        setArtifactsError(err instanceof IpcError ? err.message : String(err));
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

  const handleGenerate = useCallback(
    (artifactType: GenerationArtifactType) => {
      if (project === null || activeProvider === null) return;
      const model = activeProvider.defaultModel;
      if (typeof model !== 'string' || model.length === 0) return;
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
          upsertArtifact({
            id: detail.id,
            projectId: detail.projectId,
            artifactType: detail.artifactType,
            title: detail.title,
            status: detail.status,
            version: detail.version,
            parentId: detail.parentId ?? null,
            createdAt: detail.createdAt,
            updatedAt: detail.updatedAt,
            provider: detail.provider,
            model: detail.model,
          });
          setGeneration({ status: 'idle' });
        } catch (err) {
          setGeneration({
            status: 'error',
            message: err instanceof IpcError ? err.message : String(err),
          });
        }
      })();
    },
    [project, activeProvider, setGeneration, upsertArtifact],
  );

  const handleApprove = useCallback(
    (artifact: ArtifactSummary) => {
      void (async () => {
        try {
          await artifactsIpc.approveArtifact(artifact.id);
          upsertArtifact({ ...artifact, status: 'approved' });
        } catch (err) {
          setArtifactsError(err instanceof IpcError ? err.message : String(err));
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
          setArtifactsError(err instanceof IpcError ? err.message : String(err));
        }
      })();
    },
    [setArtifactsError, upsertArtifact],
  );

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-border px-3 py-2 flex items-center justify-between">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          AI
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

      <div className="border-b border-border p-3 space-y-3">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground">
            Provider
          </p>
          {activeProvider === null ? (
            <p className="text-xs text-muted-foreground">
              None configured. Open Settings to add a provider.
            </p>
          ) : (
            <p className="text-xs">
              <span className="font-medium">{activeProvider.provider}</span>
              {typeof activeProvider.defaultModel === 'string' &&
              activeProvider.defaultModel.length > 0 ? (
                <span className="text-muted-foreground">
                  {' · '}
                  {activeProvider.defaultModel}
                </span>
              ) : null}
            </p>
          )}
        </div>

        <div>
          <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground mb-1.5">
            Generate
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
              <pre className="bg-muted text-muted-foreground max-h-32 overflow-y-auto rounded p-2 font-mono text-[10px] leading-snug">
                {trimPartialPreview(generationStatus.partial)}
              </pre>
            ) : null}
          </div>
        ) : generationStatus.status === 'error' ? (
          <p className="text-destructive text-xs" role="alert">
            {generationStatus.message}
          </p>
        ) : null}
      </div>

      <div className="flex-1 overflow-y-auto p-3">
        <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground mb-2">
          Review queue ({reviewQueue.length})
        </p>
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
        ) : (
          <ul className="space-y-2">
            {reviewQueue.map((a) => (
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
  return (
    <li className="rounded-md border border-border bg-card p-2 text-xs">
      <button
        type="button"
        onClick={() => onOpen(artifact)}
        className="hover:bg-muted/30 -m-2 mb-0 block w-[calc(100%+1rem)] rounded-t-md p-2 text-left"
        aria-label={`Open ${artifact.title}`}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1">
            <p className="truncate font-medium" title={artifact.title}>
              {artifact.title}
            </p>
            <p className="text-muted-foreground mt-0.5 text-[10px]">
              {artifact.artifactType} · v{artifact.version} · {artifact.model}
            </p>
          </div>
          <StatusBadge status={artifact.status} />
        </div>
      </button>
      {isPending ? (
        <div className="mt-2 flex items-center gap-1">
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

function StatusBadge({ status }: { status: ArtifactSummary['status'] }) {
  const colors: Record<ArtifactSummary['status'], string> = {
    draft: 'bg-muted text-muted-foreground',
    in_review: 'bg-yellow-500/10 text-yellow-700 dark:text-yellow-400',
    approved: 'bg-green-500/10 text-green-700 dark:text-green-400',
    rejected: 'bg-red-500/10 text-red-700 dark:text-red-400',
  };
  return (
    <span
      className={`shrink-0 rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider ${colors[status]}`}
    >
      {status.replace('_', ' ')}
    </span>
  );
}
