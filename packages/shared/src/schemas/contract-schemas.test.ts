import { describe, expect, it } from 'vitest';

import {
  AnalysisOutcomeSchema,
  ArtifactSchema,
  CodeChunkSchema,
  ConnectionTestSchema,
  GenerateArgsSchema,
  GenerateResponseSchema,
  HealthStatusSchema,
  JWTPayloadSchema,
  LoginSchema,
  ProjectSchema,
  ProviderConfigSchema,
  ProviderConfigViewSchema,
  RegisterSchema,
  SaveProviderArgsSchema,
  UserSchema,
} from '../index';

describe('RegisterSchema', () => {
  it('accepts a valid registration payload', () => {
    const parsed = RegisterSchema.parse({
      email: 'user@example.com',
      password: 'password123',
      name: 'User',
    });
    expect(parsed.email).toBe('user@example.com');
  });
});

describe('LoginSchema', () => {
  it('accepts a valid login payload', () => {
    const parsed = LoginSchema.parse({
      email: 'user@example.com',
      password: 'secret',
    });
    expect(parsed.email).toBe('user@example.com');
  });
});

describe('JWTPayloadSchema', () => {
  it('accepts a valid JWT payload', () => {
    const parsed = JWTPayloadSchema.parse({
      sub: '123e4567-e89b-12d3-a456-426614174000',
      email: 'user@example.com',
      iat: 1,
      exp: 2,
    });
    expect(parsed.sub).toBe('123e4567-e89b-12d3-a456-426614174000');
  });
});

describe('ProviderConfigSchema', () => {
  it('accepts local Ollama config', () => {
    const parsed = ProviderConfigSchema.parse({
      provider: 'ollama',
      defaultModel: 'qwen2.5-coder:7b',
    });
    expect(parsed.provider).toBe('ollama');
  });

  it('rejects the legacy ollama-local literal', () => {
    expect(() =>
      ProviderConfigSchema.parse({
        provider: 'ollama-local',
        defaultModel: 'qwen2.5-coder:7b',
      }),
    ).toThrow();
  });
});

describe('ConnectionTestSchema', () => {
  it('accepts a connection test request', () => {
    const parsed = ConnectionTestSchema.parse({
      provider: 'openai',
      defaultModel: 'gpt-4o-mini',
    });
    expect(parsed.provider).toBe('openai');
  });
});

describe('UserSchema', () => {
  it('accepts a public user record', () => {
    const parsed = UserSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      email: 'user@example.com',
      name: 'User',
      plan: 'free',
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    });
    expect(parsed.plan).toBe('free');
  });
});

describe('ProjectSchema', () => {
  it('accepts a Phase 6 ProjectResponse payload', () => {
    const parsed = ProjectSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      name: 'demo',
      rootPath: '/tmp/demo',
      fileCount: 1,
      totalSizeBytes: 10,
      status: 'ready',
      languageBreakdown: { typescript: 1 },
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    });
    expect(parsed.status).toBe('ready');
    expect(parsed.rootPath).toBe('/tmp/demo');
  });

  it('rejects the legacy userId / totalSize shape', () => {
    expect(() =>
      ProjectSchema.parse({
        id: '123e4567-e89b-12d3-a456-426614174000',
        userId: '223e4567-e89b-12d3-a456-426614174001',
        name: 'demo',
        fileCount: 1,
        totalSize: 10,
        status: 'ready',
        languageBreakdown: {},
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
      }),
    ).toThrow();
  });

  it('accepts the four Phase 6 ProjectStatus literals', () => {
    for (const status of ['pending', 'analyzing', 'ready', 'error'] as const) {
      const parsed = ProjectSchema.parse({
        id: '123e4567-e89b-12d3-a456-426614174000',
        name: 'demo',
        rootPath: '/tmp/demo',
        fileCount: 0,
        totalSizeBytes: 0,
        status,
        languageBreakdown: {},
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
      });
      expect(parsed.status).toBe(status);
    }
  });
});

describe('HealthStatusSchema', () => {
  it('accepts the Phase 6 HealthStatus payload', () => {
    const parsed = HealthStatusSchema.parse({
      dbOk: true,
      osName: 'Windows',
      osVersion: '11',
      totalMemoryMb: 16384,
      availableMemoryMb: 8192,
      cpuCount: 8,
    });
    expect(parsed.cpuCount).toBe(8);
  });

  it('rejects negative memory values', () => {
    expect(() =>
      HealthStatusSchema.parse({
        dbOk: true,
        osName: 'X',
        osVersion: '1',
        totalMemoryMb: -1,
        availableMemoryMb: 0,
        cpuCount: 1,
      }),
    ).toThrow();
  });
});

describe('AnalysisOutcomeSchema', () => {
  it('accepts the Phase 6 AnalysisOutcome payload', () => {
    const parsed = AnalysisOutcomeSchema.parse({
      projectId: '123e4567-e89b-12d3-a456-426614174000',
      filesDiscovered: 12,
      filesParsed: 10,
      chunksCreated: 50,
      chunksEmbedded: 50,
      totalSizeBytes: 4096,
    });
    expect(parsed.chunksEmbedded).toBe(50);
  });
});

describe('GenerateArgsSchema', () => {
  it('accepts a minimum-fields request', () => {
    const parsed = GenerateArgsSchema.parse({
      projectId: '123e4567-e89b-12d3-a456-426614174000',
      projectName: 'demo',
      artifactType: 'test-plan',
      model: 'qwen2.5-coder:7b',
      provider: 'ollama',
    });
    expect(parsed.artifactType).toBe('test-plan');
  });

  it('rejects unknown artifact types', () => {
    expect(() =>
      GenerateArgsSchema.parse({
        projectId: '123e4567-e89b-12d3-a456-426614174000',
        projectName: 'demo',
        artifactType: 'unknown',
        model: 'qwen2.5-coder:7b',
        provider: 'ollama',
      }),
    ).toThrow();
  });
});

describe('GenerateResponseSchema', () => {
  it('accepts a generation result', () => {
    const parsed = GenerateResponseSchema.parse({
      artifactId: '123e4567-e89b-12d3-a456-426614174000',
      artifactType: 'context-md',
      contentMd: '# Project',
      usageInputTokens: 120,
      usageOutputTokens: 80,
    });
    expect(parsed.usageOutputTokens).toBe(80);
  });
});

describe('ProviderConfigViewSchema', () => {
  it('accepts a masked-key view', () => {
    const parsed = ProviderConfigViewSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      provider: 'openai',
      hasApiKey: true,
      baseUrl: null,
      defaultModel: 'gpt-4o',
      isActive: true,
    });
    expect(parsed.hasApiKey).toBe(true);
  });

  it('rejects payloads that try to smuggle a plaintext apiKey field', () => {
    const parsed = ProviderConfigViewSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      provider: 'openai',
      hasApiKey: true,
      isActive: true,
      apiKey: 'sk-leak',
    });
    // Zod strips unknown fields by default; verify the leaked key is gone.
    expect((parsed as Record<string, unknown>).apiKey).toBeUndefined();
  });
});

describe('SaveProviderArgsSchema', () => {
  it('accepts a save request with optional fields omitted', () => {
    const parsed = SaveProviderArgsSchema.parse({
      provider: 'ollama',
    });
    expect(parsed.provider).toBe('ollama');
  });
});

describe('CodeChunkSchema', () => {
  it('rejects endLine before startLine', () => {
    expect(() =>
      CodeChunkSchema.parse({
        id: '123e4567-e89b-12d3-a456-426614174000',
        projectId: '223e4567-e89b-12d3-a456-426614174001',
        fileId: '323e4567-e89b-12d3-a456-426614174002',
        chunkType: 'function',
        name: 'foo',
        content: 'fn foo() {}',
        startLine: 10,
        endLine: 2,
        tokenCount: 3,
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
      }),
    ).toThrow();
  });

  it('accepts the four kinds the Rust ChunkKind enum emits', () => {
    for (const chunkType of ['function', 'method', 'class', 'module'] as const) {
      const parsed = CodeChunkSchema.parse({
        id: '123e4567-e89b-12d3-a456-426614174000',
        projectId: '223e4567-e89b-12d3-a456-426614174001',
        fileId: '323e4567-e89b-12d3-a456-426614174002',
        chunkType,
        name: chunkType === 'module' ? '' : 'foo',
        content: 'snippet',
        startLine: 1,
        endLine: 5,
        tokenCount: 12,
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
      });
      expect(parsed.chunkType).toBe(chunkType);
    }
  });

  it('rejects legacy chunk kinds (block / other) that the Rust enum no longer emits', () => {
    for (const chunkType of ['block', 'other']) {
      expect(() =>
        CodeChunkSchema.parse({
          id: '123e4567-e89b-12d3-a456-426614174000',
          projectId: '223e4567-e89b-12d3-a456-426614174001',
          fileId: '323e4567-e89b-12d3-a456-426614174002',
          chunkType,
          name: 'x',
          content: 'snippet',
          startLine: 1,
          endLine: 1,
          tokenCount: 1,
          createdAt: '2026-05-03T12:00:00.000Z',
          updatedAt: '2026-05-03T12:00:00.000Z',
        }),
      ).toThrow();
    }
  });

  it('requires non-empty name for non-module chunk kinds', () => {
    expect(() =>
      CodeChunkSchema.parse({
        id: '123e4567-e89b-12d3-a456-426614174000',
        projectId: '223e4567-e89b-12d3-a456-426614174001',
        fileId: '323e4567-e89b-12d3-a456-426614174002',
        chunkType: 'function',
        name: '',
        content: 'fn foo() {}',
        startLine: 1,
        endLine: 1,
        tokenCount: 1,
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
      }),
    ).toThrow();
  });
});

describe('ArtifactSchema', () => {
  it('accepts an artifact with test plan structured data', () => {
    const parsed = ArtifactSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      projectId: '223e4567-e89b-12d3-a456-426614174001',
      type: 'test-plan',
      title: 'Plan',
      content: '# Plan',
      structuredData: {
        summary: 'S',
        objectives: ['O'],
        scopeIn: ['A'],
        scopeOut: [],
        strategy: 'T',
        environments: ['local'],
        risks: [],
        entryCriteria: [],
        exitCriteria: [],
      },
      status: 'draft',
      version: 1,
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    });
    expect(parsed.type).toBe('test-plan');
  });
});
