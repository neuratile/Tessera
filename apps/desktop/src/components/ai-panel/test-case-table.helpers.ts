import type {
  TestCaseResult,
  TestCaseResultResult,
  TestCaseResultSource,
  UpsertTestCaseResultInput,
} from '@testing-ide/shared';

/**
 * Pure helpers backing the {@link TestCaseTable} component
 * (plan/TEST_CASE_TABLE.md). Kept out of the `.tsx` so the component file
 * exports a component only (React Fast Refresh) and so this logic is unit
 * testable without rendering.
 */

/** Editable state of one case's cols 8–9, keyed by case id. */
export type CellState = {
  actualOutput: string;
  result: TestCaseResultResult;
  remarks: string;
  /** Who last wrote this outcome; `null` until a row exists. */
  source: TestCaseResultSource | null;
};

export const DEFAULT_CELL: CellState = {
  actualOutput: '',
  result: 'not_run',
  remarks: '',
  source: null,
};

/** Result dropdown options, in tester-facing order. */
export const RESULT_OPTIONS: ReadonlyArray<{ value: TestCaseResultResult; label: string }> = [
  { value: 'not_run', label: 'Not run' },
  { value: 'pass', label: 'Pass' },
  { value: 'fail', label: 'Fail' },
  { value: 'blocked', label: 'Blocked' },
];

/** Build the case-id → cell-state lookup from stored sidecar rows. */
export function buildResultMap(rows: readonly TestCaseResult[]): Record<string, CellState> {
  const map: Record<string, CellState> = {};
  for (const row of rows) {
    map[row.caseId] = {
      actualOutput: row.actualOutput ?? '',
      result: row.result,
      remarks: row.remarks ?? '',
      source: row.source,
    };
  }
  return map;
}

/**
 * Map one edited cell to the upsert wire payload. Empty strings drop to
 * `undefined` so the backend stores SQL NULL rather than an empty string.
 */
export function toUpsertInput(
  artifactId: string,
  caseId: string,
  cell: CellState,
): UpsertTestCaseResultInput {
  return {
    artifactId,
    caseId,
    actualOutput: cell.actualOutput.length > 0 ? cell.actualOutput : undefined,
    result: cell.result,
    remarks: cell.remarks.length > 0 ? cell.remarks : undefined,
  };
}

/**
 * Trailing-edge debouncer: rapid calls collapse to a single invocation
 * `delay` ms after the last call. One instance is kept per case so an
 * edit to one row never cancels another row's pending save.
 */
export function createDebouncer<A extends unknown[]>(
  delay: number,
  fn: (...args: A) => void,
): (...args: A) => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return (...args: A) => {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      fn(...args);
    }, delay);
  };
}
