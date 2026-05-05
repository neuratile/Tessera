import { describe, expect, it } from 'vitest';

import { ALL_TIERS, recommendTier } from './hardware-tier';

describe('recommendTier', () => {
  it('returns the entry tier below 14 GB RAM', () => {
    expect(recommendTier({ totalMemoryMb: 8 * 1024 }).id).toBe('low');
    expect(recommendTier({ totalMemoryMb: 12 * 1024 }).id).toBe('low');
  });

  it('returns the standard tier between 14 GB and 23 GB RAM', () => {
    expect(recommendTier({ totalMemoryMb: 14 * 1024 }).id).toBe('mid');
    expect(recommendTier({ totalMemoryMb: 16 * 1024 }).id).toBe('mid');
    expect(recommendTier({ totalMemoryMb: 23 * 1024 }).id).toBe('mid');
  });

  it('returns the performant tier between 24 GB and 31 GB RAM', () => {
    expect(recommendTier({ totalMemoryMb: 24 * 1024 }).id).toBe('high');
    expect(recommendTier({ totalMemoryMb: 28 * 1024 }).id).toBe('high');
  });

  it('caps at high tier even on workstation-class memory until GPU detection lands', () => {
    // Phase 8: no GPU detection — auto-select must NOT push users onto
    // the 32B model based on RAM alone. Settings UI lets them upgrade.
    expect(recommendTier({ totalMemoryMb: 64 * 1024 }).id).toBe('high');
    expect(recommendTier({ totalMemoryMb: 128 * 1024 }).id).toBe('high');
  });
});

describe('ALL_TIERS', () => {
  it('exposes every tier with a non-empty model and rationale', () => {
    expect(ALL_TIERS).toHaveLength(4);
    for (const tier of ALL_TIERS) {
      expect(tier.recommendedModel.length).toBeGreaterThan(0);
      expect(tier.rationale.length).toBeGreaterThan(0);
    }
  });
});
