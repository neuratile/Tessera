import type { FlakyRunResult } from '@testing-ide/shared';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';

// Hoisted mutable state the store mocks read, so each test can vary opt-in and
// the flaky result without depending on real store instances under SSR render.
const { uiState, sandboxState } = vi.hoisted(() => {
  const sandboxState: {
    byArtifact: Record<string, unknown>;
    flakyByArtifact: Record<string, unknown>;
  } = { byArtifact: {}, flakyByArtifact: {} };
  return { uiState: { sandboxOptIn: true }, sandboxState };
});

// The panel reaches the backend through the IPC barrel; mock it so a node-env
// render never touches the Tauri bridge (callbacks never fire during render).
vi.mock('@/lib/ipc', () => ({
  getErrorMessage: (e: unknown) => String(e),
  sandbox: {
    runTestSandbox: vi.fn(),
    runTestSandboxFlaky: vi.fn(),
    cancelTestSandbox: vi.fn(),
  },
}));

vi.mock('@/stores/ui-store', () => ({
  useUiStore: (sel: (s: { sandboxOptIn: boolean }) => unknown) => sel(uiState),
}));

vi.mock('@/stores/sandbox-store', () => {
  const IDLE = { phase: 'idle', clientRunId: null, result: null, error: null };
  const noop = () => {};
  return {
    IDLE_RUN: IDLE,
    IDLE_FLAKY: IDLE,
    useSandboxStore: (sel: (s: Record<string, unknown>) => unknown) =>
      sel({
        byArtifact: sandboxState.byArtifact,
        flakyByArtifact: sandboxState.flakyByArtifact,
        start: noop,
        finish: noop,
        fail: noop,
        reset: noop,
        startFlaky: noop,
        finishFlaky: noop,
        failFlaky: noop,
      }),
  };
});

import { SandboxRunPanel } from './sandbox-run-panel';

const ARTIFACT_ID = '123e4567-e89b-12d3-a456-426614174000';

function render() {
  return renderToStaticMarkup(<SandboxRunPanel artifactId={ARTIFACT_ID} hasFiles={true} />);
}

afterEach(() => {
  uiState.sandboxOptIn = true;
  sandboxState.byArtifact = {};
  sandboxState.flakyByArtifact = {};
});

describe('SandboxRunPanel — flaky check', () => {
  it('offers Check flaky + a runs stepper when opted in and runnable', () => {
    const html = render();
    expect(html).toContain('Check flaky');
    expect(html).toContain('Runs');
    // Default iteration count is surfaced in the stepper.
    expect(html).toContain('>5<');
    // Helper copy is shown while idle.
    expect(html).toContain('runs the suite N times');
  });

  it('renders the summary and a flaky row with its ratio + sample failure', () => {
    const result: FlakyRunResult = {
      runId: ARTIFACT_ID,
      totalRuns: 5,
      flakyCount: 1,
      nonFlakyCount: 1,
      tests: [
        {
          name: 'TC-LOGIN-01 accepts valid credentials',
          verdict: 'stable_pass',
          passCount: 5,
          executedCount: 5,
          totalRuns: 5,
        },
        {
          name: 'TC-CART-09 computes tax',
          verdict: 'flaky',
          passCount: 4,
          executedCount: 5,
          totalRuns: 5,
          sampleFailure: 'expected 19.99 to equal 20.00',
        },
      ],
    };
    sandboxState.flakyByArtifact = {
      [ARTIFACT_ID]: { phase: 'done', clientRunId: null, result, error: null },
    };

    const html = render();
    expect(html).toContain('1 of 2 tests flaky');
    expect(html).toContain('passed 4/5');
    expect(html).toContain('expected 19.99 to equal 20.00');
    expect(html).toContain('TC-CART-09 computes tax');
  });

  it('shows the error message and no verdict rows for a failed check', () => {
    const result: FlakyRunResult = {
      runId: '',
      totalRuns: 5,
      flakyCount: 0,
      nonFlakyCount: 0,
      tests: [],
      errorMessage: 'Flaky check failed on run 2 of 5: [DOCKER_UNAVAILABLE] docker unavailable',
    };
    sandboxState.flakyByArtifact = {
      [ARTIFACT_ID]: { phase: 'done', clientRunId: null, result, error: null },
    };

    const html = render();
    expect(html).toContain('DOCKER_UNAVAILABLE');
    expect(html).not.toContain('tests flaky');
  });
});
