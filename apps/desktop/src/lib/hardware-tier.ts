import type { HealthStatus } from '@testing-ide/shared';

/**
 * Local-model recommendation tiers — derived from the README "Hardware
 * tier → recommended model" table. Values picked for consumer hardware
 * compatibility per `plan/initial-plan.md`.
 *
 * The wizard surfaces this at first launch so users on a 16 GB laptop
 * don't try to pull a 32B model that won't fit in memory.
 */
export type HardwareTier = {
  /** Stable id used in telemetry / settings persistence. */
  id: 'low' | 'mid' | 'high' | 'workstation';
  /** Human-readable label rendered in the wizard. */
  label: string;
  /** Default Ollama model tag for this tier. */
  recommendedModel: string;
  /** One-line rationale shown under the recommendation. */
  rationale: string;
};

const TIERS: Record<HardwareTier['id'], HardwareTier> = {
  low: {
    id: 'low',
    label: 'Entry (≤ 12 GB RAM)',
    recommendedModel: 'qwen2.5-coder:1.5b',
    rationale:
      'Smallest code-tuned model — runs CPU-only on modest hardware. Slower but functional.',
  },
  mid: {
    id: 'mid',
    label: 'Standard (16 GB RAM)',
    recommendedModel: 'qwen2.5-coder:7b',
    rationale: 'Project default. Apache-2.0, 128K context, runs on 8 GB VRAM or 16 GB RAM CPU.',
  },
  high: {
    id: 'high',
    label: 'Performant (32 GB RAM, mid GPU)',
    recommendedModel: 'qwen2.5-coder:14b',
    rationale: 'Better quality, fits in ~12-16 GB VRAM (RTX 4070 Ti / M2 Pro).',
  },
  workstation: {
    id: 'workstation',
    label: 'Workstation (32 GB+ RAM, 24 GB VRAM)',
    recommendedModel: 'qwen2.5-coder:32b',
    rationale: 'Near GPT-4 quality on code. Needs RTX 4090 / M3 Max 64 GB or equivalent.',
  },
};

/**
 * Map a backend `HealthStatus` to a recommended tier.
 *
 * The mapping uses total memory only — the desktop app does not have GPU
 * detection in Phase 8 (would require either WebGPU probing or a Tauri
 * command around `wgpu` / `nvml`). Tier breakpoints align with the README
 * table; see the inline comments.
 */
export function recommendTier(health: Pick<HealthStatus, 'totalMemoryMb'>): HardwareTier {
  const gb = health.totalMemoryMb / 1024;
  if (gb >= 32) {
    // Workstation tier requires 32 GB+ AND a high-end GPU. We can't detect
    // the GPU yet, so we surface the 14B model as the safe default and let
    // power users override in Settings rather than auto-select 32B.
    return TIERS.high;
  }
  if (gb >= 24) return TIERS.high;
  if (gb >= 14) return TIERS.mid;
  return TIERS.low;
}

/** Exposed for the Settings screen so users can pick a tier manually. */
export const ALL_TIERS: ReadonlyArray<HardwareTier> = Object.values(TIERS);
