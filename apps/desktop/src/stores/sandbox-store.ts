import type { CoverageLine, RunResult } from '@testing-ide/shared';
import { create } from 'zustand';

/**
 * Sandbox test-runner UI state.
 *
 * Run state is keyed by artifact id so each test-cases artifact tracks its
 * own run independently. `coverage` holds the lines from the most recent
 * completed run so the editor can paint gutters for whichever source file is
 * open (the editor matches by path suffix).
 */

export type SandboxRunPhase = 'idle' | 'running' | 'done';

export type ArtifactRunState = {
  phase: SandboxRunPhase;
  /** Correlation id of the in-flight run, used to target the Stop button. */
  clientRunId: string | null;
  result: RunResult | null;
  /** Pre-flight failure (opt-out, no runnable files, IPC error). */
  error: string | null;
};

/** Stable idle reference so selectors don't churn renders. */
export const IDLE_RUN: ArtifactRunState = {
  phase: 'idle',
  clientRunId: null,
  result: null,
  error: null,
};

export type SandboxState = {
  byArtifact: Record<string, ArtifactRunState>;
  /** Coverage from the most recent completed run (editor gutter source). */
  coverage: CoverageLine[];
  start: (artifactId: string, clientRunId: string) => void;
  finish: (artifactId: string, result: RunResult) => void;
  fail: (artifactId: string, message: string) => void;
  reset: (artifactId: string) => void;
};

export const useSandboxStore = create<SandboxState>()((set) => ({
  byArtifact: {},
  coverage: [],

  start: (artifactId, clientRunId) =>
    set((s) => ({
      byArtifact: {
        ...s.byArtifact,
        [artifactId]: { phase: 'running', clientRunId, result: null, error: null },
      },
    })),

  finish: (artifactId, result) =>
    set((s) => ({
      byArtifact: {
        ...s.byArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result, error: null },
      },
      coverage: result.coverage,
    })),

  fail: (artifactId, message) =>
    set((s) => ({
      byArtifact: {
        ...s.byArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result: null, error: message },
      },
    })),

  reset: (artifactId) =>
    set((s) => {
      if (!(artifactId in s.byArtifact)) return s;
      const { [artifactId]: _dropped, ...rest } = s.byArtifact;
      void _dropped;
      return { byArtifact: rest };
    }),
}));
