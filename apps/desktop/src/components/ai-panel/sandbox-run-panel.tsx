import type {
  FlakyCheckRecord,
  FlakyCheckSummary,
  FlakyRunResult,
  FlakyTestResult,
  HealAttempt,
  HealResult,
  MutantResult,
  MutationCheckRecord,
  MutationCheckSummary,
  MutationResult,
  RunResult,
  TestResult,
} from '@testing-ide/shared';
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FlaskConical,
  History,
  Loader2,
  Minus,
  Play,
  Plus,
  Repeat,
  Sparkles,
  Square,
  Wrench,
  XCircle,
} from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { getErrorMessage, healing, mutation, sandbox } from '@/lib/ipc';
import {
  IDLE_FLAKY,
  IDLE_HEAL,
  IDLE_MUTATION,
  IDLE_RUN,
  useSandboxStore,
} from '@/stores/sandbox-store';
import { useUiStore } from '@/stores/ui-store';

/** Iteration bounds for a flaky check — mirrors the backend re-clamp. */
const FLAKY_MIN_RUNS = 2;
const FLAKY_MAX_RUNS = 20;
const FLAKY_DEFAULT_RUNS = 5;

const clampRuns = (n: number): number =>
  Math.min(FLAKY_MAX_RUNS, Math.max(FLAKY_MIN_RUNS, Math.round(n)));

/** Attempt bounds for a self-heal — mirrors the backend re-clamp [1, 5]. */
const HEAL_MIN_ATTEMPTS = 1;
const HEAL_MAX_ATTEMPTS = 5;
const HEAL_DEFAULT_ATTEMPTS = 3;

const clampAttempts = (n: number): number =>
  Math.min(HEAL_MAX_ATTEMPTS, Math.max(HEAL_MIN_ATTEMPTS, Math.round(n)));

/**
 * Regeneration context the self-heal loop needs to call `generate` between
 * runs. Sourced from the drawer's selected project + active provider (the same
 * inputs its manual "Regenerate" uses). `undefined` when no provider/model is
 * configured — the self-heal action is then disabled with guidance.
 */
export type HealContext = {
  projectId: string;
  projectName: string;
  model: string;
  provider: string;
};

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
  /**
   * Context for the self-heal loop's regeneration step. `undefined` disables
   * the "Generate & self-heal" action (no provider/model configured).
   */
  healContext?: HealContext | undefined;
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
export function SandboxRunPanel({ artifactId, hasFiles, healContext }: Props) {
  const optIn = useUiStore((s) => s.sandboxOptIn);
  const runnable = hasFiles !== false;
  const runState = useSandboxStore((s) => s.byArtifact[artifactId] ?? IDLE_RUN);
  const flakyState = useSandboxStore((s) => s.flakyByArtifact[artifactId] ?? IDLE_FLAKY);
  const healState = useSandboxStore((s) => s.healByArtifact[artifactId] ?? IDLE_HEAL);
  const mutationState = useSandboxStore((s) => s.mutationByArtifact[artifactId] ?? IDLE_MUTATION);
  const start = useSandboxStore((s) => s.start);
  const finish = useSandboxStore((s) => s.finish);
  const fail = useSandboxStore((s) => s.fail);
  const startFlaky = useSandboxStore((s) => s.startFlaky);
  const finishFlaky = useSandboxStore((s) => s.finishFlaky);
  const failFlaky = useSandboxStore((s) => s.failFlaky);
  const startHeal = useSandboxStore((s) => s.startHeal);
  const attemptHeal = useSandboxStore((s) => s.attemptHeal);
  const finishHeal = useSandboxStore((s) => s.finishHeal);
  const failHeal = useSandboxStore((s) => s.failHeal);
  const startMutation = useSandboxStore((s) => s.startMutation);
  const progressMutation = useSandboxStore((s) => s.progressMutation);
  const finishMutation = useSandboxStore((s) => s.finishMutation);
  const failMutation = useSandboxStore((s) => s.failMutation);

  const [runs, setRuns] = useState(FLAKY_DEFAULT_RUNS);
  const [maxAttempts, setMaxAttempts] = useState(HEAL_DEFAULT_ATTEMPTS);

  // Persisted flaky-check history (design §7). Kept in local state — it is
  // read-only fetched data scoped to this panel, so it does not belong in the
  // shared run store. Re-fetched on mount, on artifact change, and after each
  // completed check so a fresh run shows up at the top of the trend.
  const [history, setHistory] = useState<FlakyCheckSummary[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);

  // Persisted mutation-score history (design §5.5), kept separate from the flaky
  // trend. Same read-only, panel-scoped lifecycle.
  const [mutationHistory, setMutationHistory] = useState<MutationCheckSummary[]>([]);
  const [mutationHistoryError, setMutationHistoryError] = useState<string | null>(null);

  // Always-current artifact id, read by in-flight history fetches to detect a
  // switch. If the panel is reused with a new `artifactId` before a previous
  // `listFlakyChecks` settles, its resolution must not write the old artifact's
  // history into the new one (same race the per-row detail guards against).
  const artifactIdRef = useRef(artifactId);
  artifactIdRef.current = artifactId;

  const refreshHistory = useCallback(() => {
    void (async () => {
      try {
        const checks = await sandbox.listFlakyChecks(artifactId);
        if (artifactIdRef.current !== artifactId) return; // artifact switched mid-fetch
        setHistory(checks);
        setHistoryError(null);
      } catch (err) {
        if (artifactIdRef.current !== artifactId) return;
        setHistoryError(getErrorMessage(err));
      }
    })();
  }, [artifactId]);

  const refreshMutationHistory = useCallback(() => {
    void (async () => {
      try {
        const checks = await mutation.listMutationChecks(artifactId);
        if (artifactIdRef.current !== artifactId) return; // artifact switched mid-fetch
        setMutationHistory(checks);
        setMutationHistoryError(null);
      } catch (err) {
        if (artifactIdRef.current !== artifactId) return;
        setMutationHistoryError(getErrorMessage(err));
      }
    })();
  }, [artifactId]);

  // Clear stale history immediately on an artifact switch so the previous
  // artifact's trends never linger while the new fetches are in flight.
  useEffect(() => {
    setHistory([]);
    setHistoryError(null);
    setMutationHistory([]);
    setMutationHistoryError(null);
    refreshHistory();
    refreshMutationHistory();
  }, [refreshHistory, refreshMutationHistory]);

  const running = runState.phase === 'running';
  const flakyRunning = flakyState.phase === 'running';
  const healRunning = healState.phase === 'running';
  const mutationRunning = mutationState.phase === 'running';
  const busy = running || flakyRunning || healRunning || mutationRunning;
  const gated = !optIn || !runnable;
  // Self-heal also needs a regeneration context (provider + model).
  const healGated = gated || healContext === undefined;

  // Cancel whichever op is in flight when the panel unmounts (e.g. the
  // artifact drawer closes) so the Docker container is killed immediately
  // instead of burning compute. A ref keeps the effect mount/unmount-only —
  // re-running it on every state change would cancel healthy runs.
  const inFlightRef = useRef<string | null>(null);
  inFlightRef.current = running
    ? runState.clientRunId
    : flakyRunning
      ? flakyState.clientRunId
      : healRunning
        ? healState.clientRunId
        : mutationRunning
          ? mutationState.clientRunId
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

  // Live heal progress: the backend streams one `heal://event` per attempt,
  // tagged with the `clientRunId` the heal was started under. Subscribe once
  // and route matching events to this artifact's heal slice. A ref holds the
  // current in-flight clientRunId so the listener (installed once) always sees
  // the latest value without re-subscribing.
  const healClientRunIdRef = useRef<string | null>(null);
  healClientRunIdRef.current = healRunning ? healState.clientRunId : null;
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void (async () => {
      const fn = await healing.subscribeToHealEvents((event) => {
        if (event.healId === healClientRunIdRef.current) {
          attemptHeal(artifactId, {
            attempt: event.attempt,
            passed: event.passed,
            failed: event.failed,
          });
        }
      });
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten !== null) unlisten();
    };
  }, [artifactId, attemptHeal]);

  // Live mutation progress: the backend streams one `mutation://event` per
  // mutant, tagged with the sweep's `clientRunId`. Mirrors the heal subscription.
  const mutationClientRunIdRef = useRef<string | null>(null);
  mutationClientRunIdRef.current = mutationRunning ? mutationState.clientRunId : null;
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void (async () => {
      const fn = await mutation.subscribeToMutationEvents((event) => {
        if (event.mutationId === mutationClientRunIdRef.current) {
          progressMutation(artifactId, { done: event.done, total: event.total });
        }
      });
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten !== null) unlisten();
    };
  }, [artifactId, progressMutation]);

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
        // A completed check (no pre-flight error) is persisted to history —
        // refresh so it appears at the top of the trend. An errored / cancelled
        // check writes no history row, so this is a harmless no-op there.
        if (typeof result.errorMessage !== 'string') {
          refreshHistory();
        }
      } catch (err) {
        failFlaky(artifactId, getErrorMessage(err));
      }
    })();
  }, [gated, busy, artifactId, runs, startFlaky, finishFlaky, failFlaky, refreshHistory]);

  const handleHeal = useCallback(() => {
    if (healGated || busy || healContext === undefined) return;
    const clientRunId = crypto.randomUUID();
    startHeal(artifactId, clientRunId);
    void (async () => {
      try {
        const result = await healing.runSelfHeal({
          artifactId,
          maxAttempts,
          optInConfirmed: true,
          clientRunId,
          model: healContext.model,
          provider: healContext.provider,
          projectId: healContext.projectId,
          projectName: healContext.projectName,
        });
        finishHeal(artifactId, result);
      } catch (err) {
        failHeal(artifactId, getErrorMessage(err));
      }
    })();
  }, [healGated, busy, artifactId, maxAttempts, healContext, startHeal, finishHeal, failHeal]);

  const handleMutationTest = useCallback(() => {
    if (gated || busy) return;
    const clientRunId = crypto.randomUUID();
    startMutation(artifactId, clientRunId);
    void (async () => {
      try {
        const result = await mutation.runMutationTest({
          artifactId,
          optInConfirmed: true,
          clientRunId,
        });
        finishMutation(artifactId, result);
        // A completed score is persisted to history — refresh so it appears at
        // the top of the trend.
        refreshMutationHistory();
      } catch (err) {
        failMutation(artifactId, getErrorMessage(err));
      }
    })();
  }, [gated, busy, artifactId, startMutation, finishMutation, failMutation, refreshMutationHistory]);

  // Stop targets whichever op is in flight; all share the cancel-by-id path (a
  // heal's / sweep's in-flight run is registered under its clientRunId, so Stop
  // kills the current container and the loop observes the cancelled run).
  const handleStop = useCallback(() => {
    const clientRunId = running
      ? runState.clientRunId
      : flakyRunning
        ? flakyState.clientRunId
        : healRunning
          ? healState.clientRunId
          : mutationState.clientRunId;
    if (clientRunId === null) return;
    void sandbox.cancelTestSandbox(clientRunId).catch(() => {
      // Stop is best-effort; the run still resolves and updates state.
    });
  }, [
    running,
    flakyRunning,
    healRunning,
    runState.clientRunId,
    flakyState.clientRunId,
    healState.clientRunId,
    mutationState.clientRunId,
  ]);

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
              variant="outline"
              onClick={handleMutationTest}
              disabled={gated}
              title={
                !optIn
                  ? 'Enable local test execution in Settings'
                  : !runnable
                    ? 'This artifact has no runnable files — regenerate the test cases'
                    : 'Seed bugs into the source and check how many your tests catch (mutation score)'
              }
            >
              <FlaskConical className="size-3.5" /> Mutation test
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

      {!busy ? (
        <div className="flex items-center justify-end gap-2">
          <HealAttemptsStepper
            attempts={maxAttempts}
            setAttempts={setMaxAttempts}
            disabled={healGated}
          />
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={handleHeal}
            disabled={healGated}
            title={
              !optIn
                ? 'Enable local test execution in Settings'
                : !runnable
                  ? 'This artifact has no runnable files — regenerate the test cases'
                  : healContext === undefined
                    ? 'Configure an LLM provider and model to enable self-heal'
                    : 'Run the suite, then feed failures back to the model to fix failing tests automatically'
            }
          >
            <Wrench className="size-3.5" /> Generate &amp; self-heal
          </Button>
        </div>
      ) : null}

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

      {optIn && runnable && !busy ? (
        <p className="text-muted-foreground text-[10px]">
          <strong className="font-semibold">Generate &amp; self-heal</strong> runs the suite, then
          feeds failures back to the model to fix the failing tests automatically. Bounded retries;
          stops when all pass or it stops improving.
        </p>
      ) : null}

      {optIn && runnable && !busy ? (
        <p className="text-muted-foreground text-[10px]">
          <strong className="font-semibold">Mutation test</strong> seeds small bugs into the source
          and reruns your suite — a high mutation score means your tests would actually catch those
          bugs. Needs an all-green suite first.
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

      {healRunning ? (
        <p className="text-muted-foreground flex items-center gap-2 text-xs">
          <Loader2 className="size-3 animate-spin" />
          {healState.progress !== null
            ? `Self-healing · attempt ${healState.progress.attempt} of ${maxAttempts} · ${healState.progress.failed} ${healState.progress.failed === 1 ? 'test' : 'tests'} still failing…`
            : 'Self-healing · running the suite…'}
        </p>
      ) : null}

      {healState.error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {healState.error}
        </p>
      ) : null}

      {healState.result !== null ? <HealResultView result={healState.result} /> : null}

      {mutationRunning ? (
        <p className="text-muted-foreground flex items-center gap-2 text-xs">
          <Loader2 className="size-3 animate-spin" />
          {mutationState.progress !== null
            ? `Mutation testing · mutant ${mutationState.progress.done} of ${mutationState.progress.total}…`
            : 'Mutation testing · running the baseline…'}
        </p>
      ) : null}

      {mutationState.error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {mutationState.error}
        </p>
      ) : null}

      {mutationState.result !== null ? <MutationResultView result={mutationState.result} /> : null}

      <FlakyHistorySection artifactId={artifactId} history={history} error={historyError} />

      <MutationHistorySection
        artifactId={artifactId}
        history={mutationHistory}
        error={mutationHistoryError}
      />
    </div>
  );
}

/**
 * Collapsible "Flaky history" trend for an artifact (design §7). Lists past
 * checks newest-first; expanding a row lazily fetches that check's per-test
 * verdicts and renders them with the same {@link FlakyRow} used for a live
 * check. Hidden entirely when there is no history yet, so the panel stays
 * quiet until a check has actually been run.
 */
export function FlakyHistorySection({
  artifactId,
  history,
  error,
}: {
  artifactId: string;
  history: FlakyCheckSummary[];
  error: string | null;
}) {
  // Detail / error are tagged with the check id they belong to. Expanding row
  // A then row B fires two `getFlakyCheck` calls; if A resolves *after* B is
  // expanded, its `setDetail` would otherwise populate B's row with A's data.
  // Keying the payload by check id lets the render gate on a match, so a
  // late-arriving fetch for a no-longer-expanded row is simply ignored.
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<{ checkId: string; record: FlakyCheckRecord } | null>(null);
  const [detailError, setDetailError] = useState<{ checkId: string; message: string } | null>(null);

  // Collapse any open row when the artifact changes — a stale detail from a
  // different artifact must never render.
  useEffect(() => {
    setExpandedId(null);
    setDetail(null);
    setDetailError(null);
  }, [artifactId]);

  const handleToggle = useCallback(
    (checkId: string) => {
      if (expandedId === checkId) {
        setExpandedId(null);
        return;
      }
      setExpandedId(checkId);
      setDetail(null);
      setDetailError(null);
      void (async () => {
        try {
          const record = await sandbox.getFlakyCheck(checkId);
          setDetail({ checkId, record });
        } catch (err) {
          setDetailError({ checkId, message: getErrorMessage(err) });
        }
      })();
    },
    [expandedId],
  );

  if (error !== null) {
    return (
      <p className="text-muted-foreground text-[10px]" role="alert">
        Could not load flaky history: {error}
      </p>
    );
  }
  if (history.length === 0) return null;

  return (
    <div className="space-y-1.5 border-t border-border pt-2" data-testid="flaky-history">
      <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
        <History className="size-3" /> Flaky history
      </div>
      <ul className="space-y-1">
        {history.map((check) => (
          <FlakyHistoryRow
            key={check.id}
            check={check}
            expanded={expandedId === check.id}
            detail={detail?.checkId === check.id ? detail.record : null}
            detailError={detailError?.checkId === check.id ? detailError.message : null}
            onToggle={handleToggle}
          />
        ))}
      </ul>
    </div>
  );
}

/** Format a flaky-check timestamp; falls back to the raw string if unparseable. */
function formatCheckTime(createdAt: string): string {
  const ms = Date.parse(createdAt);
  if (Number.isNaN(ms)) return createdAt;
  return new Date(ms).toLocaleString();
}

function FlakyHistoryRow({
  check,
  expanded,
  detail,
  detailError,
  onToggle,
}: {
  check: FlakyCheckSummary;
  expanded: boolean;
  detail: FlakyCheckRecord | null;
  detailError: string | null;
  onToggle: (checkId: string) => void;
}) {
  const total = check.flakyCount + check.nonFlakyCount;
  return (
    <li className="rounded border border-border bg-surface-3 text-[11px]">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-2 py-1.5 text-left hover:bg-surface-2"
        onClick={() => onToggle(check.id)}
        aria-expanded={expanded}
      >
        {expanded ? (
          <ChevronDown className="size-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3 shrink-0 text-muted-foreground" />
        )}
        <span className={`pill pill-${check.flakyCount > 0 ? 'rejected' : 'approved'}`}>
          {check.flakyCount} of {total} flaky
        </span>
        <span className="min-w-0 flex-1 truncate text-muted-foreground font-mono text-[10px]">
          {check.totalRuns} runs
        </span>
        <span className="text-muted-foreground text-[10px]">{formatCheckTime(check.createdAt)}</span>
      </button>

      {expanded ? (
        <div className="border-t border-border px-2 py-1.5">
          {detailError !== null ? (
            <p className="text-destructive text-[10px]" role="alert">
              {detailError}
            </p>
          ) : detail === null ? (
            <p className="text-muted-foreground flex items-center gap-2 text-[10px]">
              <Loader2 className="size-3 animate-spin" /> Loading…
            </p>
          ) : detail.tests.length > 0 ? (
            <ul className="space-y-1">
              {detail.tests.map((t, i) => (
                <FlakyRow key={`${t.name}-${i}`} test={t} />
              ))}
            </ul>
          ) : (
            <p className="text-muted-foreground text-[10px]">No per-test detail recorded.</p>
          )}
        </div>
      ) : null}
    </li>
  );
}

/** −/+ stepper for the self-heal attempt budget, clamped to [1, 5]. */
function HealAttemptsStepper({
  attempts,
  setAttempts,
  disabled,
}: {
  attempts: number;
  setAttempts: (n: number) => void;
  disabled: boolean;
}) {
  return (
    <div
      className="flex items-center gap-1 rounded border border-border bg-surface-3 px-1.5 py-0.5"
      title="Maximum self-heal attempts (1–5)"
    >
      <span className="text-muted-foreground text-[10px] uppercase tracking-wide">Attempts</span>
      <button
        type="button"
        className="text-muted-foreground hover:text-foreground disabled:opacity-40"
        onClick={() => setAttempts(clampAttempts(attempts - 1))}
        disabled={disabled || attempts <= HEAL_MIN_ATTEMPTS}
        aria-label="Fewer attempts"
      >
        <Minus className="size-3" />
      </button>
      <span className="w-4 text-center font-mono text-[11px] tabular-nums text-foreground">
        {attempts}
      </span>
      <button
        type="button"
        className="text-muted-foreground hover:text-foreground disabled:opacity-40"
        onClick={() => setAttempts(clampAttempts(attempts + 1))}
        disabled={disabled || attempts >= HEAL_MAX_ATTEMPTS}
        aria-label="More attempts"
      >
        <Plus className="size-3" />
      </button>
    </div>
  );
}

/** A test that was involved in the heal, derived from the per-attempt trail. */
type HealRowData = {
  name: string;
  healed: boolean;
  /** Attempt the test first passed (only when `healed`). */
  healedAtAttempt: number | null;
  /** Per-attempt failure messages, oldest first. */
  trail: { attempt: number; message: string | null }[];
};

/**
 * Derive per-test rows from a heal's attempt trail. `HealResult` only carries
 * the *failing* tests per attempt (not the always-passing ones), so this lists
 * the tests that were involved in the heal: a test absent from the landed
 * attempt's failures flipped to passing (healed at the next attempt); one still
 * present is a likely real source bug. Tests that never failed are reflected in
 * the summary count, not as rows.
 *
 * The "landed" attempt is the one the backend chose as final (`finalArtifactId`)
 * — the healed attempt, or for `exhausted`/`no_progress` the *best* attempt by
 * pass count, which is not necessarily the chronologically last one. A later
 * attempt can regress, so deriving the failing set from the last attempt would
 * mislabel a test that fails only in that worse, discarded run.
 */
function deriveHealRows(result: HealResult): HealRowData[] {
  const attempts: HealAttempt[] = result.attempts;
  const finalAttempt =
    attempts.find((a) => a.artifactId === result.finalArtifactId) ??
    attempts[attempts.length - 1];
  if (finalAttempt === undefined) return [];
  const finalFailing = new Set(finalAttempt.failures.map((f) => f.name));

  // Only attempts up to the landed one describe the artifact on screen; later
  // (discarded) attempts must not contribute rows or trail entries.
  const order: string[] = [];
  const trails = new Map<string, { attempt: number; message: string | null }[]>();
  for (const attempt of attempts) {
    if (attempt.attempt > finalAttempt.attempt) break;
    for (const failure of attempt.failures) {
      let trail = trails.get(failure.name);
      if (trail === undefined) {
        trail = [];
        trails.set(failure.name, trail);
        order.push(failure.name);
      }
      trail.push({ attempt: attempt.attempt, message: failure.failureMessage ?? null });
    }
  }

  return order.map((name) => {
    const trail = trails.get(name) ?? [];
    const stillFailing = finalFailing.has(name);
    const lastFailAttempt = trail[trail.length - 1]?.attempt ?? 0;
    return {
      name,
      healed: !stillFailing,
      healedAtAttempt: stillFailing ? null : lastFailAttempt + 1,
      trail,
    };
  });
}

/** Headline for a settled heal: outcome + attempts + pass ratio. */
function healSummary(result: HealResult): { label: string; ok: boolean } {
  const total = result.passedCount + result.failedCount;
  const ratio = `${result.passedCount}/${total} passing`;
  switch (result.outcome) {
    case 'healed':
      return { label: `healed in ${result.attemptsUsed} ${result.attemptsUsed === 1 ? 'attempt' : 'attempts'} · ${ratio}`, ok: true };
    case 'exhausted':
      return { label: `stopped after ${result.attemptsUsed} ${result.attemptsUsed === 1 ? 'attempt' : 'attempts'} · ${ratio}`, ok: false };
    case 'no_progress':
      return { label: `stopped — no progress · ${ratio}`, ok: false };
    case 'error':
      return { label: 'self-heal stopped on an error', ok: false };
  }
}

/**
 * Results of a self-heal: a top summary ("healed in 2 attempts · 14/14
 * passing"), then a per-test row for each test that was involved — healed ones
 * badged with the attempt they flipped on, still-failing ones flagged as a
 * likely real source bug, each with a collapsible per-attempt failure trail.
 */
function HealResultView({ result }: { result: HealResult }) {
  if (result.outcome === 'error') {
    return (
      <p className="text-destructive text-[11px]" data-testid="heal-results" role="alert">
        {typeof result.errorMessage === 'string' && result.errorMessage.length > 0
          ? result.errorMessage
          : 'Self-heal stopped on an error.'}
      </p>
    );
  }

  const summary = healSummary(result);
  const rows = deriveHealRows(result);
  return (
    <div className="space-y-2" data-testid="heal-results">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className={`pill pill-${summary.ok ? 'approved' : 'rejected'}`}>Self-heal</span>
        <span className="text-muted-foreground font-mono text-[10px]">{summary.label}</span>
      </div>

      {rows.length > 0 ? (
        <ul className="space-y-1">
          {rows.map((row, i) => (
            <HealRow key={`${row.name}-${i}`} row={row} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function HealRow({ row }: { row: HealRowData }) {
  return (
    <li className="rounded border border-border bg-surface-3 px-2 py-1.5 text-[11px]">
      <div className="flex items-center gap-2">
        {row.healed ? (
          <CheckCircle2 className="size-3 shrink-0 text-success" />
        ) : (
          <XCircle className="text-destructive size-3 shrink-0" />
        )}
        <span className="min-w-0 flex-1 truncate text-foreground" title={row.name}>
          {row.name}
        </span>
        {row.healed ? (
          <span className="border-success/35 bg-success/15 text-success inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[9px] font-semibold uppercase tracking-wide">
            <Sparkles className="size-2.5" /> healed · attempt {row.healedAtAttempt}
          </span>
        ) : (
          <span className="pill pill-rejected">likely real bug</span>
        )}
      </div>
      {row.trail.length > 0 ? (
        <div className="mt-1 space-y-0.5">
          {row.trail.map((step, i) => (
            <pre
              key={i}
              className="text-destructive/90 overflow-x-auto whitespace-pre-wrap break-words font-mono text-[10px]"
            >
              attempt {step.attempt}: {step.message ?? '(no failure message captured)'}
            </pre>
          ))}
        </div>
      ) : null}
    </li>
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

/** Mutation score as a whole-number percentage (e.g. 0.78 → "78%"). */
function mutationScorePct(score: number): string {
  return `${Math.round(score * 100)}%`;
}

/** A short, human hint for why a survivor matters, keyed by the operator kind. */
function survivorHint(operatorId: string): string {
  switch (operatorId) {
    case 'arithmetic':
      return 'arithmetic not asserted';
    case 'relational':
      return 'boundary not tested';
    case 'logical':
      return 'branch not exercised';
    case 'boolean_literal':
      return 'branch not exercised';
    case 'return_negation':
      return 'return value not asserted';
    default:
      return 'not caught';
  }
}

/**
 * Results of a mutation test: a top "Mutation score · 78% · killed 31/40"
 * summary, then the survivor list — the seeded bugs the suite did *not* catch,
 * each as `file:line  original → replacement  (why)`. A score with no survivors
 * is celebrated; a sweep that found no mutable operators says so.
 */
function MutationResultView({ result }: { result: MutationResult }) {
  const scorable = result.killed + result.survived;
  const survivors = result.mutants.filter((m) => m.status === 'survived');
  const ok = result.survived === 0 && result.total > 0;
  return (
    <div className="space-y-2" data-testid="mutation-results">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className={`pill pill-${result.total === 0 ? 'draft' : ok ? 'approved' : 'rejected'}`}>
          Mutation score{result.total > 0 ? ` · ${mutationScorePct(result.score)}` : ''}
        </span>
        <span className="text-muted-foreground font-mono text-[10px]">
          {result.total === 0
            ? 'no mutable operators on covered lines'
            : `killed ${result.killed}/${scorable} mutants${
                result.errored > 0 ? ` · ${result.errored} errored` : ''
              }${result.droppedCount > 0 ? ` · ${result.droppedCount} sampled out` : ''}`}
        </span>
      </div>

      {result.total > 0 && survivors.length === 0 ? (
        <p className="text-success text-[11px]">
          Every seeded bug was caught — your tests are load-bearing here.
        </p>
      ) : null}

      {survivors.length > 0 ? (
        <>
          <p className="text-muted-foreground text-[10px]">
            Survived ({survivors.length}) — bugs your tests would miss:
          </p>
          <ul className="space-y-1">
            {survivors.map((m, i) => (
              <MutationMutantRow key={`${m.mutant.file}-${m.mutant.line}-${i}`} result={m} />
            ))}
          </ul>
        </>
      ) : null}
    </div>
  );
}

/** One mutant row: status icon + `file:line  original → replacement  (hint)`. */
function MutationMutantRow({ result }: { result: MutantResult }) {
  const { mutant, status } = result;
  return (
    <li className="rounded border border-border bg-surface-3 px-2 py-1.5 text-[11px]">
      <div className="flex items-center gap-2">
        {status === 'killed' ? (
          <CheckCircle2 className="size-3 shrink-0 text-success" />
        ) : status === 'survived' ? (
          <XCircle className="text-destructive size-3 shrink-0" />
        ) : (
          <span className="bg-surface-2 size-3 shrink-0 rounded-full" aria-hidden="true" />
        )}
        <span className="min-w-0 flex-1 truncate font-mono text-foreground" title={mutant.file}>
          {mutant.file}:{mutant.line}
        </span>
        <span className="text-muted-foreground font-mono text-[10px]">
          {mutant.original} → {mutant.replacement}
        </span>
        {status === 'survived' ? (
          <span className="text-muted-foreground text-[10px]">{survivorHint(mutant.operatorId)}</span>
        ) : status === 'errored' ? (
          <span className="text-muted-foreground text-[10px]">errored</span>
        ) : null}
      </div>
    </li>
  );
}

/**
 * Collapsible "Mutation history" trend for an artifact (design §5.5). Lists past
 * scores newest-first; expanding a row lazily fetches that check's per-mutant
 * detail. Hidden entirely until a check has been run. Mirrors
 * {@link FlakyHistorySection}.
 */
export function MutationHistorySection({
  artifactId,
  history,
  error,
}: {
  artifactId: string;
  history: MutationCheckSummary[];
  error: string | null;
}) {
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<{ checkId: string; record: MutationCheckRecord } | null>(null);
  const [detailError, setDetailError] = useState<{ checkId: string; message: string } | null>(null);

  useEffect(() => {
    setExpandedId(null);
    setDetail(null);
    setDetailError(null);
  }, [artifactId]);

  const handleToggle = useCallback(
    (checkId: string) => {
      if (expandedId === checkId) {
        setExpandedId(null);
        return;
      }
      setExpandedId(checkId);
      setDetail(null);
      setDetailError(null);
      void (async () => {
        try {
          const record = await mutation.getMutationCheck(checkId);
          setDetail({ checkId, record });
        } catch (err) {
          setDetailError({ checkId, message: getErrorMessage(err) });
        }
      })();
    },
    [expandedId],
  );

  if (error !== null) {
    return (
      <p className="text-muted-foreground text-[10px]" role="alert">
        Could not load mutation history: {error}
      </p>
    );
  }
  if (history.length === 0) return null;

  return (
    <div className="space-y-1.5 border-t border-border pt-2" data-testid="mutation-history">
      <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
        <History className="size-3" /> Mutation history
      </div>
      <ul className="space-y-1">
        {history.map((check) => (
          <MutationHistoryRow
            key={check.id}
            check={check}
            expanded={expandedId === check.id}
            detail={detail?.checkId === check.id ? detail.record : null}
            detailError={detailError?.checkId === check.id ? detailError.message : null}
            onToggle={handleToggle}
          />
        ))}
      </ul>
    </div>
  );
}

function MutationHistoryRow({
  check,
  expanded,
  detail,
  detailError,
  onToggle,
}: {
  check: MutationCheckSummary;
  expanded: boolean;
  detail: MutationCheckRecord | null;
  detailError: string | null;
  onToggle: (checkId: string) => void;
}) {
  const scorable = check.killed + check.survived;
  return (
    <li className="rounded border border-border bg-surface-3 text-[11px]">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-2 py-1.5 text-left hover:bg-surface-2"
        onClick={() => onToggle(check.id)}
        aria-expanded={expanded}
      >
        {expanded ? (
          <ChevronDown className="size-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3 shrink-0 text-muted-foreground" />
        )}
        <span className={`pill pill-${check.survived === 0 && check.total > 0 ? 'approved' : 'rejected'}`}>
          {check.total > 0 ? mutationScorePct(check.score) : '—'}
        </span>
        <span className="min-w-0 flex-1 truncate text-muted-foreground font-mono text-[10px]">
          killed {check.killed}/{scorable}
        </span>
        <span className="text-muted-foreground text-[10px]">{formatCheckTime(check.createdAt)}</span>
      </button>

      {expanded ? (
        <div className="border-t border-border px-2 py-1.5">
          {detailError !== null ? (
            <p className="text-destructive text-[10px]" role="alert">
              {detailError}
            </p>
          ) : detail === null ? (
            <p className="text-muted-foreground flex items-center gap-2 text-[10px]">
              <Loader2 className="size-3 animate-spin" /> Loading…
            </p>
          ) : detail.mutants.length > 0 ? (
            <ul className="space-y-1">
              {detail.mutants.map((m, i) => (
                <MutationMutantRow key={`${m.mutant.file}-${m.mutant.line}-${i}`} result={m} />
              ))}
            </ul>
          ) : (
            <p className="text-muted-foreground text-[10px]">No per-mutant detail recorded.</p>
          )}
        </div>
      ) : null}
    </li>
  );
}
