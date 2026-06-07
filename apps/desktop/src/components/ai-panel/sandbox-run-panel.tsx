import type { RunResult, TestResult } from '@testing-ide/shared';
import { CheckCircle2, Loader2, Play, Square, XCircle } from 'lucide-react';
import { useCallback, useEffect, useRef } from 'react';

import { Button } from '@/components/ui/button';
import { getErrorMessage, sandbox } from '@/lib/ipc';
import { IDLE_RUN, useSandboxStore } from '@/stores/sandbox-store';
import { useUiStore } from '@/stores/ui-store';

type Props = {
  /** The test-cases artifact id (a UUID). */
  artifactId: string;
  /**
   * Whether the artifact's `structured_data` carries a non-empty
   * runnable `files[]` workspace. `false` disables Run with guidance
   * (the backend would reject the run anyway); `undefined` (unknown —
   * e.g. a v1 payload the Zod mirror rejects) keeps Run enabled and
   * lets the backend decide.
   */
  hasFiles?: boolean | undefined;
};

/**
 * Run + results panel for a Test Cases artifact (sandbox runner Phase 5).
 *
 * The Run button is gated on the local-execution opt-in (Settings) — off by
 * default per the "no code execution on the default path" guarantee. A run
 * registers a `clientRunId` so the Stop button can cancel it before the
 * (blocking) run IPC returns. A Docker-unavailable / failed run is not an
 * exception; it returns a `RunResult` with `status: 'error'`.
 */
export function SandboxRunPanel({ artifactId, hasFiles }: Props) {
  const optIn = useUiStore((s) => s.sandboxOptIn);
  const runnable = hasFiles !== false;
  const runState = useSandboxStore((s) => s.byArtifact[artifactId] ?? IDLE_RUN);
  const start = useSandboxStore((s) => s.start);
  const finish = useSandboxStore((s) => s.finish);
  const fail = useSandboxStore((s) => s.fail);

  const running = runState.phase === 'running';

  // Cancel an in-flight run when the panel unmounts (e.g. the artifact
  // drawer closes) so the Docker container is killed immediately instead of
  // burning compute until it finishes or times out. A ref keeps the effect
  // mount/unmount-only — re-running it on every state change would cancel
  // healthy runs.
  const inFlightRef = useRef<string | null>(null);
  inFlightRef.current = running ? runState.clientRunId : null;
  useEffect(
    () => () => {
      const clientRunId = inFlightRef.current;
      if (clientRunId !== null) {
        void sandbox.cancelTestSandbox(clientRunId).catch(() => {
          // Best-effort: the blocking run IPC still resolves and settles state.
        });
      }
    },
    [],
  );

  const handleRun = useCallback(() => {
    if (!optIn || !runnable) return;
    const clientRunId = crypto.randomUUID();
    start(artifactId, clientRunId);
    void (async () => {
      try {
        const result = await sandbox.runTestSandbox({
          artifactId,
          optInConfirmed: true,
          clientRunId,
        });
        finish(artifactId, result);
      } catch (err) {
        fail(artifactId, getErrorMessage(err));
      }
    })();
  }, [optIn, runnable, artifactId, start, finish, fail]);

  const handleStop = useCallback(() => {
    if (runState.clientRunId === null) return;
    void sandbox.cancelTestSandbox(runState.clientRunId).catch(() => {
      // Stop is best-effort; the run still resolves and updates state.
    });
  }, [runState.clientRunId]);

  return (
    <div className="space-y-2 rounded-md border border-border bg-background p-3">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
          Sandbox run
        </span>
        {running ? (
          <Button type="button" size="sm" variant="outline" onClick={handleStop} className="ml-auto">
            <Square className="size-3.5" /> Stop
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={handleRun}
            disabled={!optIn || !runnable}
            className="ml-auto"
            title={
              !optIn
                ? 'Enable local test execution in Settings'
                : !runnable
                  ? 'This artifact has no runnable files — regenerate the test cases'
                  : 'Run tests in the local Docker sandbox'
            }
          >
            <Play className="size-3.5" /> Run
          </Button>
        )}
      </div>

      {!optIn ? (
        <p className="text-muted-foreground text-[10px]">
          Local test execution is off. Enable it in Settings to run these tests in a Docker sandbox.
        </p>
      ) : null}

      {optIn && !runnable ? (
        <p className="text-muted-foreground text-[10px]">
          This artifact carries no runnable <code className="font-mono">files[]</code> workspace,
          so there is nothing to execute. Regenerate the test cases to produce one.
        </p>
      ) : null}

      {running ? (
        <p className="text-muted-foreground flex items-center gap-2 text-xs">
          <Loader2 className="size-3 animate-spin" /> Running tests in sandbox…
        </p>
      ) : null}

      {runState.error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {runState.error}
        </p>
      ) : null}

      {runState.result !== null ? <RunResultView result={runState.result} /> : null}
    </div>
  );
}

function RunResultView({ result }: { result: RunResult }) {
  const isError = result.status === 'error';
  return (
    <div className="space-y-2" data-testid="sandbox-results">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className={`pill pill-${result.status === 'passed' ? 'approved' : result.status === 'failed' || isError ? 'rejected' : 'draft'}`}>
          {result.status}
        </span>
        <span className="text-muted-foreground font-mono text-[10px]">
          {result.passedCount}/{result.passedCount + result.failedCount} passed · {result.durationMs}ms
          · {result.coverage.length} covered lines
        </span>
      </div>

      {isError && typeof result.errorMessage === 'string' ? (
        <p className="text-destructive text-[11px]" role="alert">
          {result.errorMessage}
        </p>
      ) : null}

      {result.tests.length > 0 ? (
        <ul className="space-y-1">
          {result.tests.map((t, i) => (
            <TestRow key={`${t.name}-${i}`} test={t} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function TestRow({ test }: { test: TestResult }) {
  const failed = test.status === 'failed';
  return (
    <li className="rounded border border-border bg-surface-3 px-2 py-1.5 text-[11px]">
      <div className="flex items-center gap-2">
        {test.status === 'passed' ? (
          <CheckCircle2 className="size-3 shrink-0 text-success" />
        ) : failed ? (
          <XCircle className="text-destructive size-3 shrink-0" />
        ) : (
          <span className="bg-surface-2 size-3 shrink-0 rounded-full" aria-hidden="true" />
        )}
        <span className="min-w-0 flex-1 truncate text-foreground" title={test.name}>
          {test.name}
        </span>
        {typeof test.sourceLine === 'number' ? (
          <span className="text-muted-foreground font-mono text-[10px]">L{test.sourceLine}</span>
        ) : null}
        <span className="text-muted-foreground font-mono text-[10px]">{test.durationMs}ms</span>
      </div>
      {failed && typeof test.failureMessage === 'string' ? (
        <pre className="text-destructive/90 mt-1 overflow-x-auto whitespace-pre-wrap break-words font-mono text-[10px]">
          {test.failureMessage}
        </pre>
      ) : null}
    </li>
  );
}
