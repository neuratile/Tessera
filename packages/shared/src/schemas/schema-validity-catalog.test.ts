import { describe, expect, it } from 'vitest';
import { z } from 'zod';

import {
  AnalysisOutcomeSchema,
  ArtifactDetailSchema,
  ArtifactLifecycleStatusSchema,
  ArtifactSchema,
  ArtifactStatusSchema,
  ArtifactSummarySchema,
  ArtifactTypeSchema,
  ChunkTypeSchema,
  CodeChunkSchema,
  ConnectionTestResultSchema,
  ConnectionTestSchema,
  DefectReportSchema,
  DefectSeveritySchema,
  EmbeddingVectorSchema,
  GenerateArgsSchema,
  GenerateResponseSchema,
  GenerationArtifactTypeSchema,
  HardwareInfoSchema,
  HealthStatusSchema,
  JWTPayloadSchema,
  LanguageBreakdownSchema,
  LlmProviderIdSchema,
  LoginSchema,
  ProjectSchema,
  ProjectStatusSchema,
  ProviderConfigSchema,
  ProviderConfigViewSchema,
  ProviderConnectionTestArgsSchema,
  ProviderConnectionTestResultSchema,
  RecommendedHardwareModelSchema,
  RegisterSchema,
  SaveProviderArgsSchema,
  StructuredDataSchema,
  TestCasePrioritySchema,
  TestCaseSchema,
  TestPlanSchema,
  UserRecordSchema,
  UserSchema,
} from '../index';

const UUID_A = '123e4567-e89b-12d3-a456-426614174000';
const UUID_B = '223e4567-e89b-12d3-a456-426614174001';
const UUID_C = '323e4567-e89b-12d3-a456-426614174002';
const ISO_TIMESTAMP = '2026-05-03T12:00:00.000Z';

function expectInvalid(schema: z.ZodTypeAny, value: unknown): void {
  expect(schema.safeParse(value).success).toBe(false);
}

describe('Schema validity catalog', () => {
  it('validates auth schemas', () => {
    expect(RegisterSchema.parse({ email: 'user@example.com', password: 'password123' }).email).toBe(
      'user@example.com',
    );
    expect(LoginSchema.parse({ email: 'user@example.com', password: 'secret' }).password).toBe(
      'secret',
    );
    expect(
      JWTPayloadSchema.parse({
        sub: UUID_A,
        email: 'user@example.com',
        iat: 1,
        exp: 2,
      }).sub,
    ).toBe(UUID_A);

    expectInvalid(RegisterSchema, { email: 'bad', password: 'short' });
    expectInvalid(LoginSchema, { email: 'user@example.com', password: '' });
    expectInvalid(JWTPayloadSchema, { sub: 'not-a-uuid', email: 'user@example.com', iat: 1, exp: 2 });
  });

  it('validates provider identifier and config schemas', () => {
    expect(LlmProviderIdSchema.parse('ollama-cloud')).toBe('ollama-cloud');
    expect(
      ProviderConfigSchema.parse({
        provider: 'openrouter',
        apiKey: 'sk-or-test',
        baseUrl: 'https://openrouter.ai/api',
        defaultModel: 'openai/gpt-4o-mini',
      }).provider,
    ).toBe('openrouter');
    expect(
      SaveProviderArgsSchema.parse({
        provider: 'ollama',
        baseUrl: 'http://localhost:11434',
      }).provider,
    ).toBe('ollama');
    expect(
      ProviderConfigViewSchema.parse({
        id: UUID_A,
        provider: 'openai',
        hasApiKey: true,
        baseUrl: null,
        defaultModel: 'gpt-4o-mini',
        isActive: true,
      }).id,
    ).toBe(UUID_A);

    expectInvalid(LlmProviderIdSchema, 'ollama-local');
    expectInvalid(ProviderConfigSchema, {
      provider: 'openai',
      baseUrl: 'not-a-url',
      defaultModel: 'gpt-4o',
    });
    expectInvalid(SaveProviderArgsSchema, { provider: 'legacy-provider' });
    expectInvalid(ProviderConfigViewSchema, {
      id: 'not-a-uuid',
      provider: 'openai',
      hasApiKey: false,
      isActive: true,
    });
  });

  it('validates provider connection schemas (canonical and compatibility aliases)', () => {
    const validArgs = {
      provider: 'openai',
      apiKey: 'sk-test',
      baseUrl: 'https://api.openai.com',
      defaultModel: 'gpt-4o-mini',
    };
    const validResult = {
      ok: true,
      message: 'Connection successful.',
      latencyMs: 25,
      models: ['gpt-4o-mini'],
    };

    expect(ConnectionTestSchema.parse(validArgs).provider).toBe('openai');
    expect(ProviderConnectionTestArgsSchema.parse(validArgs).defaultModel).toBe('gpt-4o-mini');
    expect(ConnectionTestResultSchema.parse(validResult).models).toContain('gpt-4o-mini');
    expect(ProviderConnectionTestResultSchema.parse(validResult).latencyMs).toBe(25);

    expectInvalid(ConnectionTestSchema, {
      provider: 'openai',
      defaultModel: '',
    });
    expectInvalid(ProviderConnectionTestArgsSchema, {
      provider: 'not-a-provider',
    });
    expectInvalid(ConnectionTestResultSchema, {
      ok: true,
      message: 'ok',
      latencyMs: 5,
    });
    expectInvalid(ProviderConnectionTestResultSchema, {
      ok: true,
      message: 'ok',
      latencyMs: -1,
      models: [],
    });
  });

  it('validates project and analysis schemas', () => {
    expect(ProjectStatusSchema.parse('ready')).toBe('ready');
    expect(LanguageBreakdownSchema.parse({ typescript: 4 }).typescript).toBe(4);
    expect(
      ProjectSchema.parse({
        id: UUID_A,
        name: 'demo',
        rootPath: '/tmp/demo',
        fileCount: 4,
        totalSizeBytes: 1024,
        status: 'ready',
        languageBreakdown: { typescript: 4 },
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).rootPath,
    ).toBe('/tmp/demo');
    expect(
      AnalysisOutcomeSchema.parse({
        projectId: UUID_A,
        filesDiscovered: 12,
        filesParsed: 10,
        chunksCreated: 20,
        chunksEmbedded: 18,
        totalSizeBytes: 4096,
      }).filesDiscovered,
    ).toBe(12);

    expectInvalid(ProjectStatusSchema, 'queued');
    expectInvalid(LanguageBreakdownSchema, { typescript: -1 });
    expectInvalid(ProjectSchema, {
      id: UUID_A,
      name: 'demo',
      rootPath: '',
      fileCount: 4,
      totalSizeBytes: 1024,
      status: 'ready',
      languageBreakdown: {},
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expectInvalid(AnalysisOutcomeSchema, {
      projectId: UUID_A,
      filesDiscovered: -1,
      filesParsed: 10,
      chunksCreated: 20,
      chunksEmbedded: 18,
      totalSizeBytes: 4096,
    });
  });

  it('validates artifact generation request/response schemas', () => {
    expect(GenerationArtifactTypeSchema.parse('context-md')).toBe('context-md');
    expect(
      GenerateArgsSchema.parse({
        projectId: UUID_A,
        projectName: 'demo',
        artifactType: 'test-plan',
        model: 'qwen2.5-coder:7b',
        provider: 'ollama',
      }).model,
    ).toBe('qwen2.5-coder:7b');
    expect(
      GenerateResponseSchema.parse({
        generationId: '8a3e4567-e89b-12d3-a456-426614174999',
        artifactId: UUID_B,
        artifactType: 'context-md',
        contentMd: '# Demo',
        usageInputTokens: 12,
        usageOutputTokens: 8,
      }).artifactId,
    ).toBe(UUID_B);

    expectInvalid(GenerationArtifactTypeSchema, 'review-queue');
    expectInvalid(GenerateArgsSchema, {
      projectId: UUID_A,
      projectName: 'demo',
      artifactType: 'test-plan',
      model: '',
      provider: 'ollama',
    });
    expectInvalid(GenerateResponseSchema, {
      artifactId: UUID_B,
      artifactType: 'context-md',
      contentMd: '# Demo',
      usageInputTokens: 12,
      usageOutputTokens: -1,
    });
  });

  it('validates hardware and health schemas', () => {
    expect(RecommendedHardwareModelSchema.parse('qwen2.5-coder:14b')).toBe('qwen2.5-coder:14b');
    expect(
      HardwareInfoSchema.parse({
        ramGb: 32,
        gpuVramGb: 24,
        gpuName: 'NVIDIA RTX 4090',
        recommendedModel: 'qwen2.5-coder:32b',
      }).recommendedModel,
    ).toBe('qwen2.5-coder:32b');
    expect(
      HealthStatusSchema.parse({
        dbOk: true,
        osName: 'Windows',
        osVersion: '11',
        totalMemoryMb: 16384,
        availableMemoryMb: 8192,
        cpuCount: 8,
      }).cpuCount,
    ).toBe(8);

    expectInvalid(RecommendedHardwareModelSchema, 'qwen2.5-coder:70b');
    expectInvalid(HardwareInfoSchema, {
      ramGb: 32,
      gpuVramGb: 24,
      gpuName: 'NVIDIA RTX 4090',
      recommendedModel: 'qwen2.5-coder:70b',
    });
    expectInvalid(HealthStatusSchema, {
      dbOk: true,
      osName: 'Windows',
      osVersion: '11',
      totalMemoryMb: -1,
      availableMemoryMb: 8192,
      cpuCount: 8,
    });
  });

  it('validates chunk schemas', () => {
    expect(ChunkTypeSchema.parse('module')).toBe('module');
    expect(EmbeddingVectorSchema.parse([0, 1.5, -2]).length).toBe(3);
    expect(
      CodeChunkSchema.parse({
        id: UUID_A,
        projectId: UUID_B,
        fileId: UUID_C,
        chunkType: 'function',
        name: 'parseFile',
        content: 'function parseFile() {}',
        startLine: 1,
        endLine: 4,
        tokenCount: 14,
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).name,
    ).toBe('parseFile');

    expectInvalid(ChunkTypeSchema, 'block');
    expectInvalid(EmbeddingVectorSchema, [1, Number.POSITIVE_INFINITY]);
    expectInvalid(CodeChunkSchema, {
      id: UUID_A,
      projectId: UUID_B,
      fileId: UUID_C,
      chunkType: 'function',
      name: '',
      content: 'function parseFile() {}',
      startLine: 4,
      endLine: 1,
      tokenCount: 14,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
  });

  it('validates artifact summary/detail payloads and structured data variants', () => {
    expect(ArtifactLifecycleStatusSchema.parse('in_review')).toBe('in_review');
    expect(ArtifactTypeSchema.parse('bug-report')).toBe('bug-report');
    expect(ArtifactStatusSchema.parse('pending_review')).toBe('pending_review');
    const structuredData = StructuredDataSchema.parse({
      summary: 'Summary',
      objectives: ['Objective'],
      scope: { inScope: ['API'], outOfScope: [] },
      strategy: 'Risk based',
      testLevels: ['unit'],
      testTypes: ['functional'],
      environments: ['local'],
      risks: [],
      entryCriteria: [],
      exitCriteria: [],
      suspensionCriteria: [],
      deliverables: [],
    });
    expect('summary' in structuredData ? structuredData.summary : undefined).toBe('Summary');
    expect(
      ArtifactSummarySchema.parse({
        id: UUID_A,
        projectId: UUID_B,
        artifactType: 'test-plan',
        title: 'Plan v1',
        status: 'draft',
        version: 1,
        parentId: null,
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
        provider: 'ollama',
        model: 'qwen2.5-coder:7b',
      }).title,
    ).toBe('Plan v1');
    expect(
      ArtifactDetailSchema.parse({
        id: UUID_A,
        projectId: UUID_B,
        artifactType: 'test-plan',
        title: 'Plan v1',
        contentMd: '# Plan',
        structuredData: { summary: 'Summary' },
        status: 'draft',
        version: 1,
        parentId: null,
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
        provider: 'ollama',
        model: 'qwen2.5-coder:7b',
        promptVersion: 'test_plan_v1',
        inputTokens: 100,
        outputTokens: 50,
      }).promptVersion,
    ).toBe('test_plan_v1');
    expect(
      ArtifactSchema.parse({
        id: UUID_A,
        projectId: UUID_B,
        type: 'defect-report',
        title: 'Defects',
        content: '# Defects',
        structuredData: { findings: [{ severity: 'major', category: 'logic', location: 'a.ts:1', description: 'Issue' }] },
        status: 'draft',
        version: 1,
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).type,
    ).toBe('defect-report');

    expectInvalid(ArtifactLifecycleStatusSchema, 'pending_review');
    expectInvalid(ArtifactTypeSchema, 'context-md');
    expectInvalid(ArtifactStatusSchema, 'in_review');
    expectInvalid(StructuredDataSchema, ['not-an-object']);
    expectInvalid(ArtifactSummarySchema, {
      id: UUID_A,
      projectId: UUID_B,
      artifactType: 'test-plan',
      title: 'Plan v1',
      status: 'draft',
      version: 0,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
      provider: 'ollama',
      model: 'qwen2.5-coder:7b',
    });
    expectInvalid(ArtifactDetailSchema, {
      id: UUID_A,
      projectId: UUID_B,
      artifactType: 'test-plan',
      title: 'Plan v1',
      contentMd: '# Plan',
      structuredData: {},
      status: 'draft',
      version: 1,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
      provider: 'ollama',
      model: 'qwen2.5-coder:7b',
      promptVersion: 'test_plan_v1',
      inputTokens: -1,
      outputTokens: 50,
    });
    expectInvalid(ArtifactSchema, {
      id: UUID_A,
      projectId: UUID_B,
      type: 'context-md',
      title: 'Context',
      content: '# Context',
      structuredData: {},
      status: 'draft',
      version: 1,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
  });

  it('validates artifact structured-data sub-schemas', () => {
    expect(DefectSeveritySchema.parse('critical')).toBe('critical');
    expect(
      DefectReportSchema.parse({
        findings: [
          {
            id: 'DEF-PARSE-CRASH',
            severity: 'major',
            category: 'logic',
            confidence: 'high',
            location: { symbol: 'parseUser', startLine: 1, endLine: 10, fileHint: 'api.ts' },
            description: 'Unvalidated JSON.parse crashes on bad input.',
            impact: 'Request handler panics.',
            fixSuggestion: 'Wrap in try/catch and return 400.',
          },
        ],
      }).findings.length,
    ).toBe(1);
    expect(TestCasePrioritySchema.parse('p1')).toBe('p1');
    expect(
      TestCaseSchema.parse({
        cases: [
          {
            id: 'TC-001',
            title: 'Login works',
            type: 'positive',
            steps: [
              { action: 'Open login page', expectedResult: 'Login form renders' },
              { action: 'Submit valid credentials', expectedResult: 'User signs in' },
            ],
            priority: 'p1',
          },
        ],
      }).cases[0]?.id,
    ).toBe('TC-001');
    expect(
      TestPlanSchema.parse({
        summary: 'Summary',
        objectives: ['Objective'],
        scope: { inScope: ['Auth'], outOfScope: ['Migrations'] },
        strategy: 'Risk based',
        testLevels: ['unit', 'e2e'],
        testTypes: ['functional', 'security'],
        environments: ['local'],
        risks: [{ description: 'Service instability', mitigation: 'Retries' }],
        entryCriteria: ['Build green'],
        exitCriteria: ['Artifacts reviewed'],
        suspensionCriteria: ['Environment outage'],
        deliverables: ['Test report'],
      }).strategy,
    ).toBe('Risk based');

    expectInvalid(DefectSeveritySchema, 'sev1');
    expectInvalid(DefectReportSchema, {
      findings: [{ severity: 'major', category: '', location: 'api.ts:42', description: 'Issue' }],
    });
    expectInvalid(TestCasePrioritySchema, 'p4');
    expectInvalid(TestCaseSchema, {
      cases: [
        {
          id: 'TC-001',
          title: 'Login works',
          type: 'positive',
          steps: [{ action: 'x', expectedResult: 'ok' }],
          priority: 'p4',
        },
      ],
    });
    expectInvalid(TestPlanSchema, {
      summary: 'Summary',
      objectives: 'not-an-array',
      scope: { inScope: [], outOfScope: [] },
      strategy: 'Risk based',
      testLevels: [],
      testTypes: [],
      environments: [],
      risks: [],
      entryCriteria: [],
      exitCriteria: [],
      suspensionCriteria: [],
      deliverables: [],
    });
  });

  it('validates user record schemas', () => {
    expect(
      UserSchema.parse({
        id: UUID_A,
        email: 'user@example.com',
        name: 'User',
        plan: 'local',
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).plan,
    ).toBe('local');
    expect(
      UserRecordSchema.parse({
        id: UUID_A,
        email: 'user@example.com',
        name: 'User',
        plan: 'local',
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
        passwordHash: '$argon2id$v=19$m=19456,t=2,p=1$abc$def',
      }).passwordHash,
    ).toContain('$argon2id');

    expectInvalid(UserSchema, {
      id: UUID_A,
      email: 'user@example.com',
      name: '',
      plan: 'local',
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expectInvalid(UserRecordSchema, {
      id: UUID_A,
      email: 'user@example.com',
      name: 'User',
      plan: 'local',
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
      passwordHash: '',
    });
  });
});
