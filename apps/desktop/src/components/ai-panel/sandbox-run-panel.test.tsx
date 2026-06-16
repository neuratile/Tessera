import type { FlakyRunResult } from '@testing-ide/shared';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';

// Hoisted mutable state the store mocks read, so each test can vary opt-in and
// the flaky result without depending on real store instances under SSR render.
const { uiState, sandboxState } = vi.hoisted(() => {
  const sandboxState: {
    byArtifact: Record<string, unknown>;
    flakyByArtifact: Record<string, unknown>;
    healByArtifact: Record<string, unknown>;
  } = { byArtifact: {}, flakyByArtifact: {}, healByArtifact: {} };
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
    listFlakyChecks: vi.fn().mockResolvedValue([]),
    getFlakyCheck: vi.fn(),
  },
  healing: {
    runSelfHeal: vi.fn(),
    subscribeToHealEvents: vi.fn().mockResolvedValue(() => {}),
  },
}));

vi.mock('@/stores/ui-store', () => ({
  useUiStore: (sel: (s: { sandboxOptIn: boolean }) => unknown) => sel(uiState),
}));

vi.mock('@/stores/sandbox-store', () => {
  const IDLE = { phase: 'idle', clientRunId: null, result: null, error: null };
  const IDLE_HEAL = { phase: 'idle', clientRunId: null, result: null, error: null, progress: null };
  const noop = () => {};
  return {
    IDLE_RUN: IDLE,
    IDLE_FLAKY: IDLE,
    IDLE_HEAL,
    useSandboxStore: (sel: (s: Record<string, unknown>) => unknown) =>
      sel({
        byArtifact: sandboxState.byArtifact,
        flakyByArtifact: sandboxState.flakyByArtifact,
        healByArtifact: sandboxState.healByArtifact,
        start: noop,
        finish: noop,
        fail: noop,
        reset: noop,
        startFlaky: noop,
        finishFlaky: noop,
        failFlaky: noop,
        startHeal: noop,
        attemptHeal: noop,
        finishHeal: noop,
        failHeal: noop,
      }),
  };
});

import type { HealContext } from './sandbox-run-panel';
import { FlakyHistorySection, SandboxRunPanel } from './sandbox-run-panel';

const ARTIFACT_ID = '123e4567-e89b-12d3-a456-426614174000';

const HEAL_CONTEXT: HealContext = {
  projectId: 'p-1',
  projectName: 'demo',
  model: 'qwen2.5-coder:7b',
  provider: 'ollama',
};

function render(healContext: HealContext | undefined = HEAL_CONTEXT) {
  return renderToStaticMarkup(
    <SandboxRunPanel artifactId={ARTIFACT_ID} hasFiles={true} healContext={healContext} />,
  );
}

afterEach(() => {
  uiState.sandboxOptIn = true;
  sandboxState.byArtifact = {};
  sandboxState.flakyByArtifact = {};
  sandboxState.healByArtifact = {};
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

describe('SandboxRunPanel — self-heal', () => {
  it('offers Generate & self-heal + an attempts stepper when opted in and runnable', () => {
    const html = render();
    expect(html).toContain('Generate &amp; self-heal');
    expect(html).toContain('Attempts');
    // Default attempt budget is surfaced in the stepper.
    expect(html).toContain('>3<');
    expect(html).toContain('feeds failures back to the model');
  });

  it('disables self-heal when no regeneration context is configured', () => {
    // Omit healContext entirely (a `render(undefined)` would hit the default).
    const html = renderToStaticMarkup(
      <SandboxRunPanel artifactId={ARTIFACT_ID} hasFiles={true} />,
    );
    expect(html).toContain('Generate &amp; self-heal');
    expect(html).toContain('Configure an LLM provider and model to enable self-heal');
  });

  it('renders a healed summary and badges the test that flipped to passing', () => {
    sandboxState.healByArtifact = {
      [ARTIFACT_ID]: {
        phase: 'done',
        clientRunId: null,
        progress: null,
        error: null,
        result: {
          outcome: 'healed',
          attemptsUsed: 2,
          finalArtifactId: 'a-2',
          finalRunId: 'r-2',
          passedCount: 14,
          failedCount: 0,
          attempts: [
            {
              attempt: 1,
              artifactId: 'a-1',
              passedCount: 13,
              failedCount: 1,
              failures: [{ name: 'TC-CART-09 computes tax', failureMessage: 'expected 19.99 to equal 20.00' }],
            },
            { attempt: 2, artifactId: 'a-2', passedCount: 14, failedCount: 0, failures: [] },
          ],
        },
      },
    };

    const html = render();
    expect(html).toContain('healed in 2 attempts · 14/14 passing');
    expect(html).toContain('TC-CART-09 computes tax');
    expect(html).toContain('healed · attempt 2');
    expect(html).toContain('attempt 1: expected 19.99 to equal 20.00');
  });

  it('flags a still-failing test as a likely real bug when exhausted', () => {
    sandboxState.healByArtifact = {
      [ARTIFACT_ID]: {
        phase: 'done',
        clientRunId: null,
        progress: null,
        error: null,
        result: {
          outcome: 'exhausted',
          attemptsUsed: 3,
          finalArtifactId: 'a-3',
          finalRunId: 'r-3',
          passedCount: 13,
          failedCount: 1,
          attempts: [
            {
              attempt: 3,
              artifactId: 'a-3',
              passedCount: 13,
              failedCount: 1,
              failures: [{ name: 'TC-CART-07 applies bulk discount', failureMessage: 'expected 45.00 to equal 50.00' }],
            },
          ],
        },
      },
    };

    const html = render();
    expect(html).toContain('stopped after 3 attempts · 13/14 passing');
    expect(html).toContain('likely real bug');
    expect(html).toContain('TC-CART-07 applies bulk discount');
  });

  it('shows the error message for an errored heal', () => {
    sandboxState.healByArtifact = {
      [ARTIFACT_ID]: {
        phase: 'done',
        clientRunId: null,
        progress: null,
        error: null,
        result: {
          outcome: 'error',
          attemptsUsed: 1,
          finalArtifactId: 'a-1',
          finalRunId: '',
          passedCount: 0,
          failedCount: 0,
          attempts: [{ attempt: 1, artifactId: 'a-1', passedCount: 0, failedCount: 0, failures: [] }],
          errorMessage: 'Self-heal cancelled during attempt 1 of 3.',
        },
      },
    };

    const html = render();
    expect(html).toContain('Self-heal cancelled during attempt 1 of 3.');
  });
});

describe('FlakyHistorySection — persisted history (design §7)', () => {
  it('renders nothing when there is no history yet', () => {
    const html = renderToStaticMarkup(
      <FlakyHistorySection artifactId={ARTIFACT_ID} history={[]} error={null} />,
    );
    expect(html).toBe('');
  });

  it('lists past checks newest-first with their flaky-count summary', () => {
    const html = renderToStaticMarkup(
      <FlakyHistorySection
        artifactId={ARTIFACT_ID}
        history={[
          {
            id: '00000000-0000-4000-8000-000000000aaa',
            runId: ARTIFACT_ID,
            totalRuns: 5,
            flakyCount: 2,
            nonFlakyCount: 10,
            createdAt: '2026-06-15T10:30:00+00:00',
          },
        ]}
        error={null}
      />,
    );
    expect(html).toContain('Flaky history');
    expect(html).toContain('2 of 12 flaky');
    expect(html).toContain('5 runs');
  });

  it('surfaces a load error instead of the trend', () => {
    const html = renderToStaticMarkup(
      <FlakyHistorySection artifactId={ARTIFACT_ID} history={[]} error={'boom'} />,
    );
    expect(html).toContain('Could not load flaky history');
    expect(html).toContain('boom');
  });
});
