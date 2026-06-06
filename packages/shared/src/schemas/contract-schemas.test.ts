import { describe, expect, it } from 'vitest';

import {
  AnalysisOutcomeSchema,
  ArtifactDetailSchema,
  ArtifactSchema,
  ArtifactSummarySchema,
  CodeChunkSchema,
  ConnectionTestSchema,
  ConnectionTestResultSchema,
  GenerateArgsSchema,
  GenerateResponseSchema,
  GenerationStreamEventSchema,
  HardwareInfoSchema,
  HealthStatusSchema,
  JWTPayloadSchema,
  LoginSchema,
  OllamaModelSchema,
  ProjectSchema,
  ProviderConfigSchema,
  ProviderConfigViewSchema,
  ProviderConnectionTestArgsSchema,
  ProviderConnectionTestResultSchema,
  RegisterSchema,
  SaveProviderArgsSchema,
  UserSchema,
  RunRequestSchema,
  TestResultSchema,
  CoverageLineSchema,
  RunResultSchema,
  TestCaseSchema,
  BugReportSchema,
  TestPlanSchema,
  DefectReportSchema,
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

  it('accepts a Gemini cloud config', () => {
    const parsed = ProviderConfigSchema.parse({
      provider: 'gemini',
      apiKey: 'test-key',
      baseUrl: 'https://generativelanguage.googleapis.com',
      defaultModel: 'gemini-2.5-flash',
    });
    expect(parsed.provider).toBe('gemini');
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

describe('ConnectionTestResultSchema', () => {
  it('accepts a successful connection result with model ids', () => {
    const parsed = ConnectionTestResultSchema.parse({
      ok: true,
      message: 'Connection successful.',
      latencyMs: 125,
      models: ['gpt-4o-mini', 'gpt-4o'],
    });
    expect(parsed.models).toContain('gpt-4o-mini');
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

describe('HardwareInfoSchema', () => {
  it('accepts the supported model recommendation literals', () => {
    const parsed = HardwareInfoSchema.parse({
      ramGb: 32,
      gpuVramGb: 24,
      gpuName: 'NVIDIA GeForce RTX 4090',
      recommendedModel: 'qwen2.5-coder:32b',
    });
    expect(parsed.recommendedModel).toBe('qwen2.5-coder:32b');
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
  it('accepts a generation result with generationId correlator', () => {
    const parsed = GenerateResponseSchema.parse({
      generationId: '8a3e4567-e89b-12d3-a456-426614174999',
      artifactId: '123e4567-e89b-12d3-a456-426614174000',
      artifactType: 'context-md',
      contentMd: '# Project',
      usageInputTokens: 120,
      usageOutputTokens: 80,
    });
    expect(parsed.generationId).toBe('8a3e4567-e89b-12d3-a456-426614174999');
    expect(parsed.usageOutputTokens).toBe(80);
  });

  it('rejects a payload missing generationId', () => {
    expect(() =>
      GenerateResponseSchema.parse({
        artifactId: '123e4567-e89b-12d3-a456-426614174000',
        artifactType: 'context-md',
        contentMd: '# Project',
        usageInputTokens: 120,
        usageOutputTokens: 80,
      }),
    ).toThrow();
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

describe('ProviderConnectionTestArgsSchema', () => {
  it('accepts an Ollama probe with no key', () => {
    const parsed = ProviderConnectionTestArgsSchema.parse({
      provider: 'ollama',
      baseUrl: 'http://localhost:11434',
    });
    expect(parsed.provider).toBe('ollama');
    expect(parsed.apiKey).toBeUndefined();
  });

  it('accepts an OpenAI probe with key', () => {
    const parsed = ProviderConnectionTestArgsSchema.parse({
      provider: 'openai',
      apiKey: 'sk-test',
    });
    expect(parsed.apiKey).toBe('sk-test');
  });
});

describe('ProviderConnectionTestResultSchema', () => {
  it('accepts a successful probe payload', () => {
    const parsed = ProviderConnectionTestResultSchema.parse({
      ok: true,
      message: 'Ollama reachable',
      latencyMs: 12,
      models: ['qwen2.5-coder:7b'],
    });
    expect(parsed.ok).toBe(true);
    expect(parsed.latencyMs).toBe(12);
  });

  it('rejects negative latency', () => {
    expect(() =>
      ProviderConnectionTestResultSchema.parse({
        ok: false,
        message: 'failed',
        latencyMs: -1,
        models: [],
      }),
    ).toThrow();
  });
});

describe('ArtifactSummarySchema', () => {
  it('accepts the Phase 11 review-queue payload', () => {
    const parsed = ArtifactSummarySchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      projectId: '223e4567-e89b-12d3-a456-426614174001',
      artifactType: 'test-plan',
      title: 'Plan v1',
      status: 'draft',
      version: 1,
      parentId: null,
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
      provider: 'ollama',
      model: 'qwen2.5-coder:7b',
    });
    expect(parsed.status).toBe('draft');
  });

  it('accepts every Phase 11 lifecycle status literal', () => {
    for (const status of ['draft', 'in_review', 'approved', 'rejected'] as const) {
      const parsed = ArtifactSummarySchema.parse({
        id: '123e4567-e89b-12d3-a456-426614174000',
        projectId: '223e4567-e89b-12d3-a456-426614174001',
        artifactType: 'test-plan',
        title: 'X',
        status,
        version: 1,
        parentId: null,
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
        provider: 'ollama',
        model: 'qwen2.5-coder:7b',
      });
      expect(parsed.status).toBe(status);
    }
  });
});

describe('GenerationStreamEventSchema', () => {
  it('accepts a tool_args delta', () => {
    const parsed = GenerationStreamEventSchema.parse({
      generationId: '8a3e4567-e89b-12d3-a456-426614174999',
      kind: 'tool_args',
      delta: '{"sum',
    });
    expect(parsed.kind).toBe('tool_args');
    expect(parsed.delta).toBe('{"sum');
  });

  it('accepts a done event with usage stats', () => {
    const parsed = GenerationStreamEventSchema.parse({
      generationId: '8a3e4567-e89b-12d3-a456-426614174999',
      kind: 'done',
      inputTokens: 1024,
      outputTokens: 512,
    });
    expect(parsed.kind).toBe('done');
    expect(parsed.outputTokens).toBe(512);
  });

  it('rejects unknown kinds', () => {
    expect(() =>
      GenerationStreamEventSchema.parse({
        generationId: '8a3e4567-e89b-12d3-a456-426614174999',
        kind: 'whatever',
      }),
    ).toThrow();
  });
});

describe('OllamaModelSchema', () => {
  it('accepts the daemon listing payload', () => {
    const parsed = OllamaModelSchema.parse({ name: 'qwen2.5-coder:7b', sizeBytes: 4_700_000_000 });
    expect(parsed.name).toBe('qwen2.5-coder:7b');
  });

  it('rejects negative sizes', () => {
    expect(() => OllamaModelSchema.parse({ name: 'x', sizeBytes: -1 })).toThrow();
  });
});

describe('ArtifactDetailSchema', () => {
  it('accepts the Phase 11 detail payload', () => {
    const parsed = ArtifactDetailSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      projectId: '223e4567-e89b-12d3-a456-426614174001',
      artifactType: 'test-plan',
      title: 'Plan v1',
      contentMd: '# Plan',
      structuredData: { summary: 'S', objectives: ['O'] },
      status: 'draft',
      version: 1,
      parentId: null,
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
      provider: 'ollama',
      model: 'qwen2.5-coder:7b',
      promptVersion: 'test_plan_v1',
      inputTokens: 120,
      outputTokens: 80,
    });
    expect(parsed.contentMd).toBe('# Plan');
    expect(parsed.outputTokens).toBe(80);
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

describe('RunRequestSchema', () => {
  it('accepts an opted-in run request', () => {
    const parsed = RunRequestSchema.parse({
      artifactId: '123e4567-e89b-12d3-a456-426614174000',
      optInConfirmed: true,
    });
    expect(parsed.optInConfirmed).toBe(true);
  });

  it('rejects a non-uuid artifactId', () => {
    expect(() => RunRequestSchema.parse({ artifactId: 'nope', optInConfirmed: true })).toThrow();
  });

  it('carries an optional clientRunId for the Stop path', () => {
    const parsed = RunRequestSchema.parse({
      artifactId: '123e4567-e89b-12d3-a456-426614174000',
      optInConfirmed: true,
      clientRunId: '8a3e4567-e89b-12d3-a456-426614174999',
    });
    expect(parsed.clientRunId).toBe('8a3e4567-e89b-12d3-a456-426614174999');
  });

  it('accepts any non-empty clientRunId — the Rust RunRequest is a plain String (§12.3.1)', () => {
    const parsed = RunRequestSchema.parse({
      artifactId: '123e4567-e89b-12d3-a456-426614174000',
      optInConfirmed: true,
      clientRunId: 'cli-run-42',
    });
    expect(parsed.clientRunId).toBe('cli-run-42');
  });

  it('rejects an empty clientRunId', () => {
    expect(() =>
      RunRequestSchema.parse({
        artifactId: '123e4567-e89b-12d3-a456-426614174000',
        optInConfirmed: true,
        clientRunId: '',
      }),
    ).toThrow();
  });
});

describe('TestResultSchema', () => {
  it('accepts a passing case without optional fields', () => {
    const parsed = TestResultSchema.parse({
      name: 'adds two numbers',
      status: 'passed',
      durationMs: 12,
    });
    expect(parsed.status).toBe('passed');
    expect(parsed.failureMessage).toBeUndefined();
  });

  it('accepts a failing case with a message and source line', () => {
    const parsed = TestResultSchema.parse({
      name: 'throws on bad input',
      status: 'failed',
      durationMs: 4,
      failureMessage: 'expected 2 to equal 3',
      sourceLine: 42,
    });
    expect(parsed.sourceLine).toBe(42);
  });

  it('rejects an unknown status', () => {
    expect(() =>
      TestResultSchema.parse({ name: 'x', status: 'errored', durationMs: 1 }),
    ).toThrow();
  });

  it('rejects a source line below 1', () => {
    expect(() =>
      TestResultSchema.parse({ name: 'x', status: 'passed', durationMs: 1, sourceLine: 0 }),
    ).toThrow();
  });
});

describe('CoverageLineSchema', () => {
  it('accepts a covered line', () => {
    const parsed = CoverageLineSchema.parse({ filePath: 'src/add.ts', line: 3, hits: 5 });
    expect(parsed.hits).toBe(5);
  });

  it('treats hits = 0 as a valid (uncovered) line', () => {
    const parsed = CoverageLineSchema.parse({ filePath: 'src/add.ts', line: 7, hits: 0 });
    expect(parsed.hits).toBe(0);
  });

  it('rejects line numbers below 1', () => {
    expect(() => CoverageLineSchema.parse({ filePath: 'a.ts', line: 0, hits: 1 })).toThrow();
  });
});

describe('RunResultSchema', () => {
  it('accepts a completed run with cases and coverage', () => {
    const parsed = RunResultSchema.parse({
      runId: '8a3e4567-e89b-12d3-a456-426614174999',
      status: 'failed',
      passedCount: 1,
      failedCount: 1,
      durationMs: 350,
      tests: [
        { name: 'a', status: 'passed', durationMs: 10 },
        { name: 'b', status: 'failed', durationMs: 4, failureMessage: 'boom', sourceLine: 12 },
      ],
      coverage: [
        { filePath: 'src/add.ts', line: 1, hits: 1 },
        { filePath: 'src/add.ts', line: 2, hits: 0 },
      ],
    });
    expect(parsed.passedCount).toBe(1);
    expect(parsed.tests).toHaveLength(2);
    expect(parsed.coverage[1]?.hits).toBe(0);
  });

  it('accepts an error run carrying an errorMessage and empty arrays', () => {
    const parsed = RunResultSchema.parse({
      runId: '8a3e4567-e89b-12d3-a456-426614174999',
      status: 'error',
      passedCount: 0,
      failedCount: 0,
      durationMs: 0,
      tests: [],
      coverage: [],
      errorMessage: 'docker daemon unreachable',
    });
    expect(parsed.status).toBe('error');
    expect(parsed.errorMessage).toContain('docker');
  });

  it('rejects an unknown run status', () => {
    expect(() =>
      RunResultSchema.parse({
        runId: '8a3e4567-e89b-12d3-a456-426614174999',
        status: 'queued',
        passedCount: 0,
        failedCount: 0,
        durationMs: 0,
        tests: [],
        coverage: [],
      }),
    ).toThrow();
  });
});

describe('TestCaseSchema', () => {
  /** Minimal valid v2 case — separated steps + case type. */
  const v2Case = {
    id: 'TC-ADD-1',
    title: 'adds two numbers',
    type: 'positive',
    priority: 'p1',
    steps: [{ action: 'call add(1, 2)', expectedResult: 'returns 3' }],
  };

  it('accepts a descriptive-only artifact (no runnable files)', () => {
    const parsed = TestCaseSchema.parse({ cases: [v2Case] });
    expect(parsed.files).toBeUndefined();
    expect(parsed.cases[0]?.steps[0]?.expectedResult).toBe('returns 3');
  });

  it('round-trips the full v2 field set', () => {
    const parsed = TestCaseSchema.parse({
      cases: [
        {
          ...v2Case,
          type: 'boundary',
          preconditions: ['add is exported'],
          testData: 'a = Number.MAX_SAFE_INTEGER, b = 1',
          postconditions: ['no global state mutated'],
          traceability: ['src/add.ts#add'],
        },
      ],
    });
    expect(parsed.cases[0]?.type).toBe('boundary');
    expect(parsed.cases[0]?.testData).toContain('MAX_SAFE_INTEGER');
    expect(parsed.cases[0]?.postconditions).toHaveLength(1);
  });

  it('rejects v1-style plain-string steps', () => {
    expect(() =>
      TestCaseSchema.parse({
        cases: [{ ...v2Case, steps: ['call add(1, 2)'] }],
      }),
    ).toThrow();
  });

  it('rejects a case missing the type discriminator', () => {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars -- destructure to omit `type`
    const { type: _type, ...withoutType } = v2Case;
    expect(() => TestCaseSchema.parse({ cases: [withoutType] })).toThrow();
  });

  it('rejects an empty steps array (minItems 1)', () => {
    expect(() => TestCaseSchema.parse({ cases: [{ ...v2Case, steps: [] }] })).toThrow();
  });

  it('rejects unknown case types', () => {
    expect(() =>
      TestCaseSchema.parse({ cases: [{ ...v2Case, type: 'smoke' }] }),
    ).toThrow();
  });

  it('accepts a runnable workspace mirroring the sandbox WorkspaceFile shape', () => {
    const parsed = TestCaseSchema.parse({
      cases: [v2Case],
      files: [
        { path: 'src/add.ts', contents: 'export const add = (a, b) => a + b;', isTest: false },
        { path: 'add.test.ts', contents: "import { test, expect } from 'vitest';", isTest: true },
      ],
    });
    expect(parsed.files).toHaveLength(2);
    expect(parsed.files?.[1]?.isTest).toBe(true);
  });

  it('rejects a file missing the isTest discriminator', () => {
    expect(() =>
      TestCaseSchema.parse({
        cases: [v2Case],
        files: [{ path: 'src/add.ts', contents: 'x' }],
      }),
    ).toThrow();
  });
});

describe('BugReportSchema', () => {
  /** Minimal valid v2 bug — split severity/priority + reproducibility. */
  const v2Bug = {
    id: 'BUG-SAVE-RACE',
    title: 'Report save races under load',
    severity: 'major',
    priority: 'p1',
    reproducibility: 'intermittent',
    stepsToReproduce: ['1. Open the app', '2. Save twice quickly'],
    expectedBehavior: 'One report row is written',
    actualBehavior: 'Two rows are written',
    rootCause: { symbol: 'saveReport', explanation: 'No write lock around the insert.' },
  };

  it('accepts a minimal v2 bug', () => {
    const parsed = BugReportSchema.parse({ bugs: [v2Bug] });
    expect(parsed.bugs[0]?.severity).toBe('major');
    expect(parsed.bugs[0]?.priority).toBe('p1');
  });

  it('round-trips the full v2 field set', () => {
    const parsed = BugReportSchema.parse({
      bugs: [
        {
          ...v2Bug,
          severity: 'blocker',
          environment: 'Windows 11 / Node 22',
          component: 'report-service',
          workaround: 'Save once and wait for the toast',
          rootCause: {
            symbol: 'saveReport',
            startLine: 10,
            endLine: 20,
            fileHint: 'src/report.ts',
            explanation: 'No write lock around the insert.',
          },
          evidenceSnippet: 'await insert(report); await insert(report);',
        },
      ],
    });
    expect(parsed.bugs[0]?.severity).toBe('blocker');
    expect(parsed.bugs[0]?.component).toBe('report-service');
    expect(parsed.bugs[0]?.rootCause.fileHint).toBe('src/report.ts');
  });

  it('rejects an empty stepsToReproduce (minItems 1)', () => {
    expect(() =>
      BugReportSchema.parse({ bugs: [{ ...v2Bug, stepsToReproduce: [] }] }),
    ).toThrow();
  });

  it('rejects v1 4-level severity value sets missing blocker', () => {
    expect(() => BugReportSchema.parse({ bugs: [{ ...v2Bug, severity: 'high' }] })).toThrow();
  });

  it('rejects a bug missing the priority split', () => {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars -- destructure to omit `priority`
    const { priority: _priority, ...withoutPriority } = v2Bug;
    expect(() => BugReportSchema.parse({ bugs: [withoutPriority] })).toThrow();
  });

  it('rejects unknown reproducibility values', () => {
    expect(() =>
      BugReportSchema.parse({ bugs: [{ ...v2Bug, reproducibility: 'sometimes' }] }),
    ).toThrow();
  });

  it('rejects a lowercase-mixed bug id', () => {
    expect(() => BugReportSchema.parse({ bugs: [{ ...v2Bug, id: 'BUG-Save-Race' }] })).toThrow();
  });

  it('rejects an inverted rootCause line range (endLine < startLine)', () => {
    expect(() =>
      BugReportSchema.parse({
        bugs: [
          {
            ...v2Bug,
            rootCause: {
              symbol: 'saveReport',
              startLine: 20,
              endLine: 10,
              explanation: 'No write lock around the insert.',
            },
          },
        ],
      }),
    ).toThrow();
  });
});

describe('TestPlanSchema', () => {
  /** Minimal valid v2 plan — nested scope + 29119 backbone sections. */
  const v2Plan = {
    summary: 'Validate the auth subsystem.',
    objectives: ['Verify login'],
    scope: { inScope: ['auth module'], outOfScope: ['migrations'] },
    strategy: 'Risk-based API checks.',
    testLevels: ['unit', 'integration'],
    testTypes: ['functional', 'security'],
    environments: ['local'],
    risks: [{ description: 'Session leak' }],
    entryCriteria: ['Build green'],
    exitCriteria: ['Cases reviewed'],
    suspensionCriteria: ['Environment outage'],
    deliverables: ['Test case suite'],
  };

  it('round-trips the full v2 field set', () => {
    const parsed = TestPlanSchema.parse(v2Plan);
    expect(parsed.scope.inScope).toEqual(['auth module']);
    expect(parsed.testLevels).toContain('integration');
    expect(parsed.suspensionCriteria).toHaveLength(1);
  });

  it('rejects v1-style flat scopeIn/scopeOut', () => {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars -- destructure to omit `scope`
    const { scope: _scope, ...flat } = v2Plan;
    expect(() =>
      TestPlanSchema.parse({ ...flat, scopeIn: ['auth module'], scopeOut: [] }),
    ).toThrow();
  });

  it('rejects unknown test levels and types', () => {
    expect(() => TestPlanSchema.parse({ ...v2Plan, testLevels: ['smoke'] })).toThrow();
    expect(() => TestPlanSchema.parse({ ...v2Plan, testTypes: ['exploratory'] })).toThrow();
  });

  it('rejects a plan missing suspensionCriteria or deliverables', () => {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars -- destructure to omit `suspensionCriteria`
    const { suspensionCriteria: _s, ...withoutSuspension } = v2Plan;
    expect(() => TestPlanSchema.parse(withoutSuspension)).toThrow();
    // eslint-disable-next-line @typescript-eslint/no-unused-vars -- destructure to omit `deliverables`
    const { deliverables: _d, ...withoutDeliverables } = v2Plan;
    expect(() => TestPlanSchema.parse(withoutDeliverables)).toThrow();
  });
});

describe('DefectReportSchema', () => {
  /** Minimal valid v2 finding — CWE category + evidence parity. */
  const v2Finding = {
    id: 'DEF-PARSE-CRASH',
    severity: 'major',
    category: 'input_validation',
    confidence: 'high',
    location: { symbol: 'parseUser', startLine: 1, endLine: 10 },
    description: 'Unvalidated JSON.parse crashes on bad input.',
    impact: 'Request handler panics.',
    fixSuggestion: 'Wrap in try/catch and return 400.',
  };

  it('round-trips the full v2 field set', () => {
    const parsed = DefectReportSchema.parse({
      findings: [
        {
          ...v2Finding,
          location: { ...v2Finding.location, fileHint: 'src/api.ts' },
          evidenceSnippet: 'return JSON.parse(s);',
        },
      ],
      summary: 'One high-confidence defect.',
    });
    expect(parsed.findings[0]?.category).toBe('input_validation');
    expect(parsed.findings[0]?.location.fileHint).toBe('src/api.ts');
    expect(parsed.findings[0]?.evidenceSnippet).toContain('JSON.parse');
  });

  it('rejects categories outside the CWE-aligned enum', () => {
    expect(() =>
      DefectReportSchema.parse({ findings: [{ ...v2Finding, category: 'null_safety' }] }),
    ).toThrow();
  });

  it('rejects a finding missing fixSuggestion', () => {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars -- destructure to omit `fixSuggestion`
    const { fixSuggestion: _f, ...withoutFix } = v2Finding;
    expect(() => DefectReportSchema.parse({ findings: [withoutFix] })).toThrow();
  });

  it('rejects v1-style string locations', () => {
    expect(() =>
      DefectReportSchema.parse({ findings: [{ ...v2Finding, location: 'api.ts:42' }] }),
    ).toThrow();
  });

  it('rejects an inverted location line range (endLine < startLine)', () => {
    expect(() =>
      DefectReportSchema.parse({
        findings: [{ ...v2Finding, location: { symbol: 'parseUser', startLine: 10, endLine: 5 } }],
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
        scope: { inScope: ['A'], outOfScope: [] },
        strategy: 'T',
        testLevels: ['unit'],
        testTypes: ['functional'],
        environments: ['local'],
        risks: [],
        entryCriteria: [],
        exitCriteria: [],
        suspensionCriteria: [],
        deliverables: [],
      },
      status: 'draft',
      version: 1,
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    });
    expect(parsed.type).toBe('test-plan');
  });
});
