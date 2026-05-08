import { describe, expect, it } from 'vitest';

import {
  HardwareInfoSchema,
  RecommendedHardwareModelSchema,
} from './hardware.schema';

describe('RecommendedHardwareModelSchema', () => {
  it('accepts the three supported model tags', () => {
    expect(RecommendedHardwareModelSchema.parse('qwen2.5-coder:7b')).toBe(
      'qwen2.5-coder:7b',
    );
    expect(RecommendedHardwareModelSchema.parse('qwen2.5-coder:14b')).toBe(
      'qwen2.5-coder:14b',
    );
    expect(RecommendedHardwareModelSchema.parse('qwen2.5-coder:32b')).toBe(
      'qwen2.5-coder:32b',
    );
  });

  it('rejects unsupported or mistyped model tags', () => {
    const invalid = [
      'qwen2.5-coder:70b',
      'qwen2.5-coder:1.5b',
      'llama3.1:8b',
      '',
      'deepseek-r1:7b',
    ];
    for (const value of invalid) {
      expect(RecommendedHardwareModelSchema.safeParse(value).success).toBe(
        false,
      );
    }
  });
});

describe('HardwareInfoSchema', () => {
  it('accepts a high-end workstation profile', () => {
    const parsed = HardwareInfoSchema.parse({
      ramGb: 64,
      gpuVramGb: 24,
      gpuName: 'NVIDIA GeForce RTX 4090',
      recommendedModel: 'qwen2.5-coder:32b',
    });
    expect(parsed.ramGb).toBe(64);
    expect(parsed.gpuVramGb).toBe(24);
    expect(parsed.gpuName).toBe('NVIDIA GeForce RTX 4090');
    expect(parsed.recommendedModel).toBe('qwen2.5-coder:32b');
  });

  it('accepts a laptop with no dedicated GPU (null fields)', () => {
    const parsed = HardwareInfoSchema.parse({
      ramGb: 16,
      gpuVramGb: null,
      gpuName: null,
      recommendedModel: 'qwen2.5-coder:7b',
    });
    expect(parsed.ramGb).toBe(16);
    expect(parsed.gpuVramGb).toBeNull();
    expect(parsed.gpuName).toBeNull();
  });

  it('rejects negative RAM', () => {
    expect(
      HardwareInfoSchema.safeParse({
        ramGb: -8,
        gpuVramGb: null,
        gpuName: null,
        recommendedModel: 'qwen2.5-coder:7b',
      }).success,
    ).toBe(false);
  });

  it('rejects an unsupported recommendedModel', () => {
    expect(
      HardwareInfoSchema.safeParse({
        ramGb: 32,
        gpuVramGb: 24,
        gpuName: 'NVIDIA RTX 4090',
        recommendedModel: 'qwen2.5-coder:70b',
      }).success,
    ).toBe(false);
  });

  it('rejects missing required fields', () => {
    expect(
      HardwareInfoSchema.safeParse({
        ramGb: 16,
        recommendedModel: 'qwen2.5-coder:7b',
      }).success,
    ).toBe(false);
  });
});
