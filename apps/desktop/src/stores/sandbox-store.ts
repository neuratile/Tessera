import type {
  CoverageLine,
  FlakyRunResult,
  HealResult,
  ImproveResult,
  MutationResult,
  RunResult,
} from '@testing-ide/shared';
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

/**
 * Flaky-check UI state, kept separate from the normal-run state so a flaky
 * check and a single run can coexist for the same artifact
 * (plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md §5.6).
 */
export type ArtifactFlakyState = {
  phase: SandboxRunPhase;
  /** Correlation id of the in-flight check, used to target the Stop button. */
  clientRunId: string | null;
  result: FlakyRunResult | null;
  /** Pre-flight failure (opt-out, no runnable files, IPC error). */
  error: string | null;
};

/** Stable idle reference for the flaky slice. */
export const IDLE_FLAKY: ArtifactFlakyState = {
  phase: 'idle',
  clientRunId: null,
  result: null,
  error: null,
};

/** Live per-attempt progress streamed on `heal://event` while a heal runs. */
export type HealAttemptProgress = { attempt: number; passed: number; failed: number };

/**
 * Self-heal UI state, kept separate from the normal-run and flaky slices so a
 * heal, a flaky check, and a single run can coexist for one artifact
 * (plan/versions/v2/v2-feature-docs/SELF_HEALING_LOOP.md §5.7). `progress`
 * carries the latest streamed attempt so the UI can show "Attempt 2 of 3 · 1
 * test still failing" before the loop settles.
 */
export type ArtifactHealState = {
  phase: SandboxRunPhase;
  /** Correlation id of the in-flight heal, used to target the Stop button. */
  clientRunId: string | null;
  result: HealResult | null;
  /** Pre-flight failure (opt-out, no runnable files, IPC error). */
  error: string | null;
  /** Latest streamed attempt while running; null before the first event. */
  progress: HealAttemptProgress | null;
};

/** Stable idle reference for the heal slice. */
export const IDLE_HEAL: ArtifactHealState = {
  phase: 'idle',
  clientRunId: null,
  result: null,
  error: null,
  progress: null,
};

/** Live per-mutant progress streamed on `mutation://event` during a sweep. */
export type MutationProgress = { done: number; total: number };

/**
 * Mutation-score UI state, kept separate from the run / flaky / heal slices so a
 * mutation test can coexist with the others for one artifact
 * (plan/versions/v2/v2-feature-docs/MUTATION_TESTING.md §5.8). `progress` carries
 * the latest streamed "mutant N of M" so the UI can show sweep progress before
 * the score settles.
 */
export type ArtifactMutationState = {
  phase: SandboxRunPhase;
  /** Correlation id of the in-flight sweep, used to target the Stop button. */
  clientRunId: string | null;
  result: MutationResult | null;
  /** Pre-flight / abort failure (opt-out, red baseline, cancellation, IPC error). */
  error: string | null;
  /** Latest streamed per-mutant progress while running; null before the first event. */
  progress: MutationProgress | null;
};

/** Stable idle reference for the mutation slice. */
export const IDLE_MUTATION: ArtifactMutationState = {
  phase: 'idle',
  clientRunId: null,
  result: null,
  error: null,
  progress: null,
};

/** Live per-attempt progress streamed on `improve://event` during an improve. */
export type ImproveProgress = { attempt: number; score: number };

/**
 * "Improve coverage" UI state (MUTATION_TESTING.md §5.8), kept separate from the
 * run / flaky / heal / mutation slices so an improve loop can coexist with the
 * others for one artifact. `progress` carries the latest streamed "attempt N ·
 * score" so the UI can show the loop advancing before it settles.
 */
export type ArtifactImproveState = {
  phase: SandboxRunPhase;
  /** Correlation id of the in-flight loop, used to target the Stop button. */
  clientRunId: string | null;
  result: ImproveResult | null;
  /** Pre-flight / abort failure (opt-out, red baseline, provider, IPC error). */
  error: string | null;
  /** Latest streamed per-attempt progress while running; null before the first event. */
  progress: ImproveProgress | null;
};

/** Stable idle reference for the improve slice. */
export const IDLE_IMPROVE: ArtifactImproveState = {
  phase: 'idle',
  clientRunId: null,
  result: null,
  error: null,
  progress: null,
};

export type SandboxState = {
  byArtifact: Record<string, ArtifactRunState>;
  flakyByArtifact: Record<string, ArtifactFlakyState>;
  healByArtifact: Record<string, ArtifactHealState>;
  mutationByArtifact: Record<string, ArtifactMutationState>;
  improveByArtifact: Record<string, ArtifactImproveState>;
  /** Coverage from the most recent completed run (editor gutter source). */
  coverage: CoverageLine[];
  start: (artifactId: string, clientRunId: string) => void;
  finish: (artifactId: string, result: RunResult) => void;
  fail: (artifactId: string, message: string) => void;
  reset: (artifactId: string) => void;
  startFlaky: (artifactId: string, clientRunId: string) => void;
  finishFlaky: (artifactId: string, result: FlakyRunResult) => void;
  failFlaky: (artifactId: string, message: string) => void;
  resetFlaky: (artifactId: string) => void;
  startHeal: (artifactId: string, clientRunId: string) => void;
  attemptHeal: (artifactId: string, progress: HealAttemptProgress) => void;
  finishHeal: (artifactId: string, result: HealResult) => void;
  failHeal: (artifactId: string, message: string) => void;
  resetHeal: (artifactId: string) => void;
  startMutation: (artifactId: string, clientRunId: string) => void;
  progressMutation: (artifactId: string, progress: MutationProgress) => void;
  finishMutation: (artifactId: string, result: MutationResult) => void;
  failMutation: (artifactId: string, message: string) => void;
  resetMutation: (artifactId: string) => void;
  startImprove: (artifactId: string, clientRunId: string) => void;
  progressImprove: (artifactId: string, progress: ImproveProgress) => void;
  finishImprove: (artifactId: string, result: ImproveResult) => void;
  failImprove: (artifactId: string, message: string) => void;
  resetImprove: (artifactId: string) => void;
};

export const useSandboxStore = create<SandboxState>()((set) => ({
  byArtifact: {},
  flakyByArtifact: {},
  healByArtifact: {},
  mutationByArtifact: {},
  improveByArtifact: {},
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

  startFlaky: (artifactId, clientRunId) =>
    set((s) => ({
      flakyByArtifact: {
        ...s.flakyByArtifact,
        [artifactId]: { phase: 'running', clientRunId, result: null, error: null },
      },
    })),

  finishFlaky: (artifactId, result) =>
    set((s) => ({
      flakyByArtifact: {
        ...s.flakyByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result, error: null },
      },
    })),

  failFlaky: (artifactId, message) =>
    set((s) => ({
      flakyByArtifact: {
        ...s.flakyByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result: null, error: message },
      },
    })),

  resetFlaky: (artifactId) =>
    set((s) => {
      if (!(artifactId in s.flakyByArtifact)) return s;
      const { [artifactId]: _dropped, ...rest } = s.flakyByArtifact;
      void _dropped;
      return { flakyByArtifact: rest };
    }),

  startHeal: (artifactId, clientRunId) =>
    set((s) => ({
      healByArtifact: {
        ...s.healByArtifact,
        [artifactId]: { phase: 'running', clientRunId, result: null, error: null, progress: null },
      },
    })),

  attemptHeal: (artifactId, progress) =>
    set((s) => {
      const current = s.healByArtifact[artifactId];
      // Ignore late events for a heal that already settled or was reset.
      if (current === undefined || current.phase !== 'running') return s;
      return {
        healByArtifact: {
          ...s.healByArtifact,
          [artifactId]: { ...current, progress },
        },
      };
    }),

  finishHeal: (artifactId, result) =>
    set((s) => ({
      healByArtifact: {
        ...s.healByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result, error: null, progress: null },
      },
    })),

  failHeal: (artifactId, message) =>
    set((s) => ({
      healByArtifact: {
        ...s.healByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result: null, error: message, progress: null },
      },
    })),

  resetHeal: (artifactId) =>
    set((s) => {
      if (!(artifactId in s.healByArtifact)) return s;
      const { [artifactId]: _dropped, ...rest } = s.healByArtifact;
      void _dropped;
      return { healByArtifact: rest };
    }),

  startMutation: (artifactId, clientRunId) =>
    set((s) => ({
      mutationByArtifact: {
        ...s.mutationByArtifact,
        [artifactId]: { phase: 'running', clientRunId, result: null, error: null, progress: null },
      },
    })),

  progressMutation: (artifactId, progress) =>
    set((s) => {
      const current = s.mutationByArtifact[artifactId];
      // Ignore late events for a sweep that already settled or was reset.
      if (current === undefined || current.phase !== 'running') return s;
      return {
        mutationByArtifact: {
          ...s.mutationByArtifact,
          [artifactId]: { ...current, progress },
        },
      };
    }),

  finishMutation: (artifactId, result) =>
    set((s) => ({
      mutationByArtifact: {
        ...s.mutationByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result, error: null, progress: null },
      },
    })),

  failMutation: (artifactId, message) =>
    set((s) => ({
      mutationByArtifact: {
        ...s.mutationByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result: null, error: message, progress: null },
      },
    })),

  resetMutation: (artifactId) =>
    set((s) => {
      if (!(artifactId in s.mutationByArtifact)) return s;
      const { [artifactId]: _dropped, ...rest } = s.mutationByArtifact;
      void _dropped;
      return { mutationByArtifact: rest };
    }),

  startImprove: (artifactId, clientRunId) =>
    set((s) => ({
      improveByArtifact: {
        ...s.improveByArtifact,
        [artifactId]: { phase: 'running', clientRunId, result: null, error: null, progress: null },
      },
    })),

  progressImprove: (artifactId, progress) =>
    set((s) => {
      const current = s.improveByArtifact[artifactId];
      // Ignore late events for an improve that already settled or was reset.
      if (current === undefined || current.phase !== 'running') return s;
      return {
        improveByArtifact: {
          ...s.improveByArtifact,
          [artifactId]: { ...current, progress },
        },
      };
    }),

  finishImprove: (artifactId, result) =>
    set((s) => ({
      improveByArtifact: {
        ...s.improveByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result, error: null, progress: null },
      },
    })),

  failImprove: (artifactId, message) =>
    set((s) => ({
      improveByArtifact: {
        ...s.improveByArtifact,
        [artifactId]: { phase: 'done', clientRunId: null, result: null, error: message, progress: null },
      },
    })),

  resetImprove: (artifactId) =>
    set((s) => {
      if (!(artifactId in s.improveByArtifact)) return s;
      const { [artifactId]: _dropped, ...rest } = s.improveByArtifact;
      void _dropped;
      return { improveByArtifact: rest };
    }),
}));
