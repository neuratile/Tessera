import type { TestCase, UpsertTestCaseResultInput } from '@testing-ide/shared';
import { useCallback, useEffect, useRef, useState } from 'react';

import {
  buildResultMap,
  type CellState,
  createDebouncer,
  type Debouncer,
  DEFAULT_CELL,
  RESULT_OPTIONS,
  toUpsertInput,
} from '@/components/ai-panel/test-case-table.helpers';
import { getErrorMessage, testCaseResults } from '@/lib/ipc';

/**
 * The fixed 9-column Test Case table (plan/TEST_CASE_TABLE.md). The LLM
 * decides how many rows (cases) to emit; the columns are fixed. Columns
 * 8–9 (Actual output / Result and remarks) are editable cells backed by
 * the `test_case_results` sidecar — a tester types them or an opt-in
 * sandbox run auto-fills them, and either survives artifact
 * regeneration because the sidecar is keyed by `(artifact_id, case_id)`.
 */

type Case = TestCase['cases'][number];

const SAVE_DEBOUNCE_MS = 500;

const HEADER_CLASS =
  'text-muted-foreground border-b border-border py-1.5 px-2 text-left text-[10px] font-semibold uppercase tracking-[0.08em] align-bottom';
const CELL_CLASS = 'border-b border-border/50 py-1.5 px-2 align-top';

function NumberedCell({ items }: { items: readonly string[] | undefined }) {
  if (items === undefined || items.length === 0) {
    return <span className="text-muted-foreground">—</span>;
  }
  return (
    <ol className="list-decimal space-y-0.5 pl-4">
      {/* Ordered, never re-ordered; items can repeat, so the index is a stable key. */}
      {items.map((item, i) => (
        <li key={i}>{item}</li>
      ))}
    </ol>
  );
}

export function TestCaseTable({ artifactId, data }: { artifactId: string; data: TestCase }) {
  const [results, setResults] = useState<Record<string, CellState>>({});
  const [showOptional, setShowOptional] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Mirror of `results` for synchronous reads inside event handlers, so a
  // burst of edits composes off the latest state without stale closures.
  const resultsRef = useRef<Record<string, CellState>>({});
  // One debouncer per case id (created lazily on first edit).
  const saversRef = useRef<Map<string, Debouncer<[UpsertTestCaseResultInput]>>>(new Map());
  // The artifact a save belongs to — guards against a stale in-flight save
  // surfacing its error on a different artifact opened mid-flight.
  const currentArtifactRef = useRef(artifactId);

  const commit = useCallback((next: Record<string, CellState>) => {
    resultsRef.current = next;
    setResults(next);
  }, []);

  // (Re)load the sidecar whenever the open artifact changes.
  useEffect(() => {
    let cancelled = false;
    currentArtifactRef.current = artifactId;
    // The savers Map identity is stable for the component's lifetime, so
    // capturing it here is safe to reuse in the cleanup closure.
    const savers = saversRef.current;
    commit({});
    // Drop any pending per-case saves from the previous artifact so their
    // timers cannot fire against the new one, then start fresh.
    savers.forEach((saver) => saver.cancel());
    savers.clear();
    void (async () => {
      try {
        const rows = await testCaseResults.listTestCaseResults(artifactId);
        if (cancelled) return;
        commit(buildResultMap(rows));
        setError(null);
      } catch (err) {
        if (!cancelled) setError(getErrorMessage(err));
      }
    })();
    return () => {
      cancelled = true;
      savers.forEach((saver) => saver.cancel());
    };
  }, [artifactId, commit]);

  const saveCell = useCallback((input: UpsertTestCaseResultInput) => {
    void testCaseResults.upsertTestCaseResult(input).catch((err: unknown) => {
      // Only surface the error if its artifact is still the one on screen —
      // an in-flight save from a since-closed artifact must stay silent.
      if (input.artifactId === currentArtifactRef.current) setError(getErrorMessage(err));
    });
  }, []);

  const scheduleSave = useCallback(
    (caseId: string, cell: CellState) => {
      let saver = saversRef.current.get(caseId);
      if (saver === undefined) {
        saver = createDebouncer(SAVE_DEBOUNCE_MS, saveCell);
        saversRef.current.set(caseId, saver);
      }
      saver(toUpsertInput(artifactId, caseId, cell));
    },
    [artifactId, saveCell],
  );

  const updateCell = useCallback(
    (caseId: string, patch: Partial<CellState>) => {
      const base = resultsRef.current[caseId] ?? DEFAULT_CELL;
      // A tester edit is, by definition, a manual-source outcome.
      const next: CellState = { ...base, ...patch, source: 'manual' };
      commit({ ...resultsRef.current, [caseId]: next });
      scheduleSave(caseId, next);
    },
    [commit, scheduleSave],
  );

  const cellOf = (caseId: string): CellState => results[caseId] ?? DEFAULT_CELL;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <p className="text-muted-foreground text-[10px] uppercase tracking-[0.12em]">
          {data.cases.length} test case{data.cases.length === 1 ? '' : 's'}
        </p>
        <label className="text-muted-foreground flex items-center gap-1.5 text-[11px]">
          <input
            type="checkbox"
            checked={showOptional}
            onChange={(e) => setShowOptional(e.target.checked)}
          />
          Show type / priority / traceability
        </label>
      </div>

      {error !== null ? (
        <p className="text-destructive text-[11px]" role="alert">
          {error}
        </p>
      ) : null}

      <div className="overflow-x-auto rounded-md border border-border">
        <table className="w-full border-collapse text-left text-xs">
          <thead>
            <tr>
              <th className={`${HEADER_CLASS} w-10`}>Sr no</th>
              <th className={HEADER_CLASS}>Test case ID</th>
              <th className={HEADER_CLASS}>Description</th>
              <th className={HEADER_CLASS}>Precondition</th>
              <th className={HEADER_CLASS}>Steps to reproduce</th>
              <th className={HEADER_CLASS}>Input steps</th>
              <th className={HEADER_CLASS}>Expected output</th>
              <th className={HEADER_CLASS}>Actual output</th>
              <th className={HEADER_CLASS}>Result and remarks</th>
              {showOptional ? (
                <>
                  <th className={HEADER_CLASS}>Type</th>
                  <th className={HEADER_CLASS}>Priority</th>
                  <th className={HEADER_CLASS}>Traceability</th>
                </>
              ) : null}
            </tr>
          </thead>
          <tbody>
            {data.cases.map((tc: Case, index) => {
              const cell = cellOf(tc.id);
              return (
                <tr key={tc.id}>
                  <td className={`${CELL_CLASS} text-muted-foreground font-mono`}>{index + 1}</td>
                  <td className={`${CELL_CLASS} font-mono`}>
                    <span className="font-semibold">{tc.id}</span>
                    {cell.source === 'sandbox' ? (
                      <span
                        className="bg-surface-2 text-muted-foreground ml-1 inline-block rounded border border-border px-1 py-0.5 text-[9px] uppercase tracking-[0.08em]"
                        title="Auto-filled by a sandbox run"
                      >
                        sandbox
                      </span>
                    ) : null}
                  </td>
                  <td className={CELL_CLASS}>{tc.title}</td>
                  <td className={CELL_CLASS}>
                    <NumberedCell items={tc.preconditions} />
                  </td>
                  <td className={CELL_CLASS}>
                    <NumberedCell items={tc.steps.map((s) => s.action)} />
                  </td>
                  <td className={`${CELL_CLASS} font-mono`}>
                    {tc.testData !== undefined && tc.testData.length > 0 ? (
                      tc.testData
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </td>
                  <td className={CELL_CLASS}>
                    <NumberedCell items={tc.steps.map((s) => s.expectedResult)} />
                  </td>
                  <td className={CELL_CLASS}>
                    <textarea
                      className="bg-surface-2 min-h-[2.5rem] w-40 rounded border border-border p-1 font-mono text-[11px]"
                      value={cell.actualOutput}
                      placeholder="—"
                      aria-label={`Actual output for ${tc.id}`}
                      onChange={(e) => updateCell(tc.id, { actualOutput: e.target.value })}
                    />
                  </td>
                  <td className={CELL_CLASS}>
                    <div className="flex flex-col gap-1">
                      <select
                        className="bg-surface-2 rounded border border-border px-1 py-0.5 text-[11px]"
                        value={cell.result}
                        aria-label={`Result for ${tc.id}`}
                        onChange={(e) => {
                          const value = RESULT_OPTIONS.find((o) => o.value === e.target.value);
                          if (value !== undefined) updateCell(tc.id, { result: value.value });
                        }}
                      >
                        {RESULT_OPTIONS.map((o) => (
                          <option key={o.value} value={o.value}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                      <input
                        className="bg-surface-2 w-40 rounded border border-border px-1 py-0.5 text-[11px]"
                        value={cell.remarks}
                        placeholder="Remarks"
                        aria-label={`Remarks for ${tc.id}`}
                        onChange={(e) => updateCell(tc.id, { remarks: e.target.value })}
                      />
                    </div>
                  </td>
                  {showOptional ? (
                    <>
                      <td className={CELL_CLASS}>{tc.type}</td>
                      <td className={CELL_CLASS}>{tc.priority}</td>
                      <td className={CELL_CLASS}>
                        <NumberedCell items={tc.traceability} />
                      </td>
                    </>
                  ) : null}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
