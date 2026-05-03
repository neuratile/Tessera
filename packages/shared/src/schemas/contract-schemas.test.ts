import { describe, expect, it } from 'vitest';

import {
  ArtifactSchema,
  CodeChunkSchema,
  ConnectionTestSchema,
  JWTPayloadSchema,
  LoginSchema,
  ProjectSchema,
  ProviderConfigSchema,
  RegisterSchema,
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
  it('accepts ollama-local config', () => {
    const parsed = ProviderConfigSchema.parse({
      provider: 'ollama-local',
      defaultModel: 'qwen2.5-coder:7b',
    });
    expect(parsed.provider).toBe('ollama-local');
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
  it('accepts a project record', () => {
    const parsed = ProjectSchema.parse({
      id: '123e4567-e89b-12d3-a456-426614174000',
      userId: '223e4567-e89b-12d3-a456-426614174001',
      name: 'demo',
      fileCount: 1,
      totalSize: 10,
      status: 'ready',
      languageBreakdown: { typescript: 1 },
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    });
    expect(parsed.status).toBe('ready');
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
