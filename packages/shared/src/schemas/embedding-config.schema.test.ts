import { describe, expect, it } from 'vitest';

import {
  EmbeddingConfigViewSchema,
  EmbeddingPresetSchema,
  EmbeddingProviderIdSchema,
  IndexStatusSchema,
  SaveEmbeddingConfigArgsSchema,
  TestEmbeddingResultSchema,
} from './embedding-config.schema';

describe('EmbeddingProviderIdSchema', () => {
  it.each(['ollama', 'ollama-cloud', 'openai', 'gemini', 'huggingface'] as const)(
    'accepts %s',
    (id) => {
      expect(EmbeddingProviderIdSchema.parse(id)).toBe(id);
    },
  );

  it('rejects LLM-only providers and unknown ids', () => {
    for (const bad of ['anthropic', 'openrouter', 'voyage', '']) {
      expect(EmbeddingProviderIdSchema.safeParse(bad).success).toBe(false);
    }
  });
});

describe('EmbeddingConfigViewSchema', () => {
  it('round-trips a Rust-shaped stored config', () => {
    // Field names/casing mirror serde `rename_all = "camelCase"` output
    // of `EmbeddingConfigView` (rules.md §12.3.1 round-trip contract).
    const fromRust = {
      id: '123e4567-e89b-12d3-a456-426614174000',
      provider: 'huggingface',
      model: 'BAAI/bge-m3',
      dimension: 1024,
      baseUrl: null,
      hasApiKey: true,
      isActive: true,
    };
    expect(EmbeddingConfigViewSchema.parse(fromRust)).toEqual(fromRust);
  });

  it('accepts the implicit default view (null id)', () => {
    const parsed = EmbeddingConfigViewSchema.parse({
      id: null,
      provider: 'ollama',
      model: 'nomic-embed-text',
      dimension: 768,
      baseUrl: null,
      hasApiKey: false,
      isActive: true,
    });
    expect(parsed.id).toBeNull();
  });

  it('rejects zero dimension', () => {
    const result = EmbeddingConfigViewSchema.safeParse({
      id: null,
      provider: 'ollama',
      model: 'nomic-embed-text',
      dimension: 0,
      hasApiKey: false,
      isActive: true,
    });
    expect(result.success).toBe(false);
  });
});

describe('SaveEmbeddingConfigArgsSchema', () => {
  it('accepts a minimal save payload', () => {
    const parsed = SaveEmbeddingConfigArgsSchema.parse({
      provider: 'openai',
      model: 'text-embedding-3-small',
      dimension: 1536,
    });
    expect(parsed.apiKey).toBeUndefined();
  });

  it('rejects dimensions outside 1..=8192 (MAX_DIMENSION mirror)', () => {
    for (const dimension of [0, 8193, -1, 1.5]) {
      const result = SaveEmbeddingConfigArgsSchema.safeParse({
        provider: 'openai',
        model: 'text-embedding-3-small',
        dimension,
      });
      expect(result.success).toBe(false);
    }
  });

  it('rejects an empty model', () => {
    const result = SaveEmbeddingConfigArgsSchema.safeParse({
      provider: 'openai',
      model: '',
      dimension: 1536,
    });
    expect(result.success).toBe(false);
  });
});

describe('TestEmbeddingResultSchema', () => {
  it('round-trips a Rust-shaped probe result', () => {
    const fromRust = { latencyMs: 412, detectedDimension: 1536 };
    expect(TestEmbeddingResultSchema.parse(fromRust)).toEqual(fromRust);
  });
});

describe('EmbeddingPresetSchema', () => {
  it('round-trips a Rust-shaped preset', () => {
    const fromRust = {
      provider: 'gemini',
      model: 'gemini-embedding-001',
      dimension: 3072,
      isDefault: true,
    };
    expect(EmbeddingPresetSchema.parse(fromRust)).toEqual(fromRust);
  });
});

describe('IndexStatusSchema', () => {
  it('round-trips a stale status', () => {
    const fromRust = {
      projectId: 'p1',
      embeddedChunks: 240,
      indexedWith: {
        provider: 'ollama-nomic-embed-text',
        model: 'nomic-embed-text',
        dimension: 768,
      },
      activeConfig: {
        provider: 'openai-text-embedding-3-small',
        model: 'text-embedding-3-small',
        dimension: 1536,
      },
      isStale: true,
    };
    expect(IndexStatusSchema.parse(fromRust)).toEqual(fromRust);
  });

  it('accepts a never-indexed project (null indexedWith)', () => {
    const parsed = IndexStatusSchema.parse({
      projectId: 'p1',
      embeddedChunks: 0,
      indexedWith: null,
      activeConfig: { provider: 'ollama-nomic-embed-text', model: 'nomic-embed-text', dimension: 768 },
      isStale: false,
    });
    expect(parsed.indexedWith).toBeNull();
  });

  it('tolerates legacy provider strings from removed providers', () => {
    const parsed = IndexStatusSchema.parse({
      projectId: 'p1',
      embeddedChunks: 3,
      indexedWith: { provider: 'voyage-legacy-model', model: 'legacy-model', dimension: 512 },
      activeConfig: { provider: 'ollama-nomic-embed-text', model: 'nomic-embed-text', dimension: 768 },
      isStale: true,
    });
    expect(parsed.indexedWith?.provider).toBe('voyage-legacy-model');
  });
});
