import type { TestCase } from '@testing-ide/shared';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// The component reaches the backend through the IPC barrel; mock it so the
// node-env render never touches the Tauri bridge.
vi.mock('@/lib/ipc', () => ({
  getErrorMessage: (e: unknown) => String(e),
  testCaseResults: {
    listTestCaseResults: vi.fn().mockResolvedValue([]),
    upsertTestCaseResult: vi.fn().mockResolvedValue(undefined),
  },
}));

import {
  buildResultMap,
  createDebouncer,
  DEFAULT_CELL,
  toUpsertInput,
} from './test-case-table.helpers';
import { TestCaseTable } from './test-case-table';

const ARTIFACT_ID = '123e4567-e89b-12d3-a456-426614174000';

const TWO_CASES: TestCase = {
  cases: [
    {
      id: 'TC-LOGIN-01',
      title: 'Valid login succeeds',
      type: 'positive',
      priority: 'p0',
      preconditions: ['User exists'],
      testData: 'user@example.com / hunter2',
      steps: [{ action: 'Submit valid creds', expectedResult: 'Redirect to home' }],
      traceability: ['src/auth.ts#login'],
    },
    {
      id: 'TC-LOGIN-02',
      title: 'Empty password rejected',
      type: 'negative',
      priority: 'p1',
      steps: [{ action: 'Submit empty password', expectedResult: 'Inline error shown' }],
    },
  ],
};

describe('buildResultMap', () => {
  it('keys cell state by case id and carries the source', () => {
    const map = buildResultMap([
      {
        id: 'r1',
        artifactId: ARTIFACT_ID,
        caseId: 'TC-LOGIN-01',
        actualOutput: 'redirected',
        result: 'pass',
        remarks: null,
        source: 'sandbox',
        runId: null,
        createdAt: '2026-06-08T00:00:00Z',
        updatedAt: '2026-06-08T00:00:00Z',
      },
    ]);
    expect(map['TC-LOGIN-01']?.source).toBe('sandbox');
    expect(map['TC-LOGIN-01']?.result).toBe('pass');
    // Null DB columns collapse to empty strings for the editable inputs.
    expect(map['TC-LOGIN-01']?.remarks).toBe('');
  });
});

describe('toUpsertInput', () => {
  it('drops empty strings to undefined so the backend stores NULL', () => {
    const input = toUpsertInput(ARTIFACT_ID, 'TC-A', DEFAULT_CELL);
    expect(input).toEqual({
      artifactId: ARTIFACT_ID,
      caseId: 'TC-A',
      actualOutput: undefined,
      result: 'not_run',
      remarks: undefined,
    });
  });

  it('passes through non-empty actual output and remarks', () => {
    const input = toUpsertInput(ARTIFACT_ID, 'TC-A', {
      actualOutput: 'got 500',
      result: 'fail',
      remarks: 'flaky',
      source: 'manual',
    });
    expect(input.actualOutput).toBe('got 500');
    expect(input.remarks).toBe('flaky');
    expect(input.result).toBe('fail');
  });
});

describe('createDebouncer', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('collapses a burst of edits into one trailing invocation', () => {
    const fn = vi.fn();
    const debounced = createDebouncer(500, fn);
    debounced('a');
    debounced('b');
    debounced('c');
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(500);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith('c');
  });

  it('cancel() drops a pending invocation so a stale save never fires', () => {
    const fn = vi.fn();
    const debounced = createDebouncer(500, fn);
    debounced('x');
    debounced.cancel();
    vi.advanceTimersByTime(500);
    expect(fn).not.toHaveBeenCalled();
  });
});

describe('TestCaseTable render', () => {
  it('renders the fixed header and one editable row per case', () => {
    const html = renderToStaticMarkup(<TestCaseTable artifactId={ARTIFACT_ID} data={TWO_CASES} />);
    // Fixed 9-column header.
    for (const header of [
      'Sr no',
      'Test case ID',
      'Description',
      'Precondition',
      'Steps to reproduce',
      'Input steps',
      'Expected output',
      'Actual output',
      'Result and remarks',
    ]) {
      expect(html).toContain(header);
    }
    // One Actual-output editor per case → N rows.
    const editors = html.match(/aria-label="Actual output for /g) ?? [];
    expect(editors).toHaveLength(2);
    expect(html).toContain('TC-LOGIN-01');
    expect(html).toContain('TC-LOGIN-02');
  });

  it('hides the optional type / priority / traceability columns by default', () => {
    const html = renderToStaticMarkup(<TestCaseTable artifactId={ARTIFACT_ID} data={TWO_CASES} />);
    expect(html).not.toContain('Traceability');
  });
});
