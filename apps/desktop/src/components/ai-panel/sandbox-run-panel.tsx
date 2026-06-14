import type { FlakyRunResult, FlakyTestResult, RunResult, TestResult } from '@testing-ide/shared';
import {
  AlertTriangle,
  CheckCircle2,
  Loader2,
  Minus,
  Play,
  Plus,
  Repeat,
  Square,
  XCircle,
} from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { getErrorMessage, sandbox } from '@/lib/ipc';
import { IDLE_FLAKY, IDLE_RUN, useSandboxStore } from '@/stores/sandbox-store';
import { useUiStore } from '@/stores/ui-store';

/** Iteration bounds for a flaky check — mirrors the backend re-clamp. */
const FLAKY_MIN_RUNS = 2;
const FLAKY_MAX_RUNS = 20;
const FLAKY_DEFAULT_RUNS = 5;

const clampRuns = (n: number): number =>
  Math.min(FLAKY_MAX_RUNS, Math.max(FLAKY_MIN_RUNS, Math.round(n)));

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
 *
 * "Check flaky" reuses the same gate + container to run the suite N times and
 * classify each test as stable / stable-fail / flaky
 * (plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md). Its state is kept
 * separate from the single-run state so both results can coexist.
 */
export function SandboxRunPanel({ artifactId, hasFiles }: Props) {
  const optIn = useUiStore((s) => s.sandboxOptIn);
  const runnable = hasFiles !== false;
  const runState = useSandboxStore((s) => s.byArtifact[artifactId] ?? IDLE_RUN);
  const flakyState = useSandboxStore((s) => s.flakyByArtifact[artifactId] ?? IDLE_FLAKY);
  const start = useSandboxStore((s) => s.start);
  const finish = useSandboxStore((s) => s.finish);
  const fail = useSandboxStore((s) => s.fail);
  const startFlaky = useSandboxStore((s) => s.startFlaky);
  const finishFlaky = useSandboxStore((s) => s.finishFlaky);
  const failFlaky = useSandboxStore((s) => s.failFlaky);

  const [runs, setRuns] = useState(FLAKY_DEFAULT_RUNS);

  const running = runState.phase === 'running';
  const flakyRunning = flakyState.phase === 'running';
  const busy = running || flakyRunning;
  const gated = !optIn || !runnable;

  // Cancel whichever op is in flight when the panel unmounts (e.g. the
  // artifact drawer closes) so the Docker container is killed immediately
  // instead of burning compute. A ref keeps the effect mount/unmount-only —
  // re-running it on every state change would cancel healthy runs.
  const inFlightRef = useRef<string | null>(null);
  inFlightRef.current = running
    ? runState.clientRunId
    : flakyRunning
      ? flakyState.clientRunId
      : null;
  useEffect(
    () => () => {
      const clientRunId = inFlightRef.current;
      if (clientRunId !== null) {
        void sandbox.cancelTestSandbox(clientRunId).catch(() => {
          // Best-effort: the blocking IPC still resolves and settles state.
        });
      }
    },
    [],
  );

  const handleRun = useCallback(() => {
    if (gated || busy) return;
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
  }, [gated, busy, artifactId, start, finish, fail]);

  const handleCheckFlaky = useCallback(() => {
    if (gated || busy) return;
    const clientRunId = crypto.randomUUID();
    startFlaky(artifactId, clientRunId);
    void (async () => {
      try {
        const result = await sandbox.runTestSandboxFlaky(
          { artifactId, optInConfirmed: true, clientRunId },
          runs,
        );
        finishFlaky(artifactId, result);
      } catch (err) {
        failFlaky(artifactId, getErrorMessage(err));
      }
    })();
  }, [gated, busy, artifactId, runs, startFlaky, finishFlaky, failFlaky]);

  // Stop targets whichever op is in flight; both share the cancel-by-id path.
  const handleStop = useCallback(() => {
    const clientRunId = running ? runState.clientRunId : flakyState.clientRunId;
    if (clientRunId === null) return;
    void sandbox.cancelTestSandbox(clientRunId).catch(() => {
      // Stop is best-effort; the run still resolves and updates state.
    });
  }, [running, runState.clientRunId, flakyState.clientRunId]);

  return (
    <div className="space-y-2 rounded-md border border-border bg-background p-3">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
          Sandbox run
        </span>
        {busy ? (
          <Button type="button" size="sm" variant="outline" onClick={handleStop} className="ml-auto">
            <Square className="size-3.5" /> Stop
          </Button>
        ) : (
          <div className="ml-auto flex items-center gap-2">
            <FlakyRunsStepper runs={runs} setRuns={setRuns} disabled={gated} />
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={handleCheckFlaky}
              disabled={gated}
              title={
                !optIn
                  ? 'Enable local test execution in Settings'
                  : !runnable
                    ? 'This artifact has no runnable files — regenerate the test cases'
                    : `Run the suite ${runs} times to find tests that pass sometimes and fail sometimes`
              }
            >
              <Repeat className="size-3.5" /> Check flaky
            </Button>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={handleRun}
              disabled={gated}
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
          </div>
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

      {optIn && runnable && !busy ? (
        <p className="text-muted-foreground text-[10px]">
          <strong className="font-semibold">Check flaky</strong> runs the suite N times to catch
          tests that pass sometimes and fail sometimes. More runs = more confidence, slower.
        </p>
      ) : null}

      {running ? (
        <p className="text-muted-foreground flex items-center gap-2 text-xs">
          <Loader2 className="size-3 animate-spin" /> Running tests in sandbox…
        </p>
      ) : null}

      {flakyRunning ? (
        <p className="text-muted-foreground flex items-center gap-2 text-xs">
          <Loader2 className="size-3 animate-spin" /> Checking for flaky tests · {runs} runs…
        </p>
      ) : null}

      {runState.error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {runState.error}
        </p>
      ) : null}

      {runState.result !== null ? <RunResultView result={runState.result} /> : null}

      {flakyState.error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {flakyState.error}
        </p>
      ) : null}

      {flakyState.result !== null ? <FlakyResultView result={flakyState.result} /> : null}
    </div>
  );
}

/** −/+ stepper for the flaky-check iteration count, clamped to [2, 20]. */
function FlakyRunsStepper({
  runs,
  setRuns,
  disabled,
}: {
  runs: number;
  setRuns: (n: number) => void;
  disabled: boolean;
}) {
  return (
    <div
      className="flex items-center gap-1 rounded border border-border bg-surface-3 px-1.5 py-0.5"
      title="Number of times the suite is run (2–20)"
    >
      <span className="text-muted-foreground text-[10px] uppercase tracking-wide">Runs</span>
      <button
        type="button"
        className="text-muted-foreground hover:text-foreground disabled:opacity-40"
        onClick={() => setRuns(clampRuns(runs - 1))}
        disabled={disabled || runs <= FLAKY_MIN_RUNS}
        aria-label="Fewer runs"
      >
        <Minus className="size-3" />
      </button>
      <span className="w-4 text-center font-mono text-[11px] tabular-nums text-foreground">
        {runs}
      </span>
      <button
        type="button"
        className="text-muted-foreground hover:text-foreground disabled:opacity-40"
        onClick={() => setRuns(clampRuns(runs + 1))}
        disabled={disabled || runs >= FLAKY_MAX_RUNS}
        aria-label="More runs"
      >
        <Plus className="size-3" />
      </button>
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

/**
 * Results of a flaky check: a top "X of Y tests flaky" summary plus a per-test
 * verdict list. An error result (an iteration failed / the check was stopped)
 * carries no verdicts — just the message.
 */
function FlakyResultView({ result }: { result: FlakyRunResult }) {
  if (typeof result.errorMessage === 'string' && result.errorMessage.length > 0) {
    return (
      <p className="text-destructive text-[11px]" data-testid="flaky-results" role="alert">
        {result.errorMessage}
      </p>
    );
  }

  const total = result.tests.length;
  return (
    <div className="space-y-2" data-testid="flaky-results">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className={`pill pill-${result.flakyCount > 0 ? 'rejected' : 'approved'}`}>
          {result.flakyCount} of {total} {total === 1 ? 'test' : 'tests'} flaky
        </span>
        <span className="text-muted-foreground font-mono text-[10px]">{result.totalRuns} runs</span>
      </div>

      {total > 0 ? (
        <ul className="space-y-1">
          {result.tests.map((t, i) => (
            <FlakyRow key={`${t.name}-${i}`} test={t} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

/** Format the "passed X/N" ratio; a test skipped in every run shows "skipped". */
function flakyRatio(test: FlakyTestResult): string {
  if (test.executedCount === 0) return 'skipped';
  return `passed ${test.passCount}/${test.executedCount}`;
}

function FlakyRow({ test }: { test: FlakyTestResult }) {
  const isFlaky = test.verdict === 'flaky';
  const isFail = test.verdict === 'stable_fail';
  const showSample = (isFlaky || isFail) && typeof test.sampleFailure === 'string';
  return (
    <li className="rounded border border-border bg-surface-3 px-2 py-1.5 text-[11px]">
      <div className="flex items-center gap-2">
        {isFlaky ? (
          <AlertTriangle className="size-3 shrink-0 text-warning" />
        ) : isFail ? (
          <XCircle className="text-destructive size-3 shrink-0" />
        ) : (
          <CheckCircle2 className="size-3 shrink-0 text-success" />
        )}
        <span className="min-w-0 flex-1 truncate text-foreground" title={test.name}>
          {test.name}
        </span>
        {isFlaky ? (
          <span className="border-warning/35 bg-warning/15 text-warning rounded-full border px-1.5 py-px text-[9px] font-semibold uppercase tracking-wide">
            flaky
          </span>
        ) : isFail ? (
          <span className="pill pill-rejected">fails</span>
        ) : (
          <span className="pill pill-approved">stable</span>
        )}
        <span className="text-muted-foreground font-mono text-[10px]">{flakyRatio(test)}</span>
      </div>
      {showSample ? (
        <pre className="text-destructive/90 mt-1 overflow-x-auto whitespace-pre-wrap break-words font-mono text-[10px]">
          {test.sampleFailure}
        </pre>
      ) : null}
    </li>
  );
}
