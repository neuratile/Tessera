import { describe, expect, it } from 'vitest';

import {
  ProjectSchema,
  ProjectStatusSchema,
  LanguageBreakdownSchema,
} from './project.schema';

const UUID_A = '123e4567-e89b-12d3-a456-426614174000';
const ISO_TIMESTAMP = '2026-05-03T12:00:00.000Z';

describe('ProjectStatusSchema', () => {
  it('accepts all four lifecycle literals', () => {
    for (const status of ['pending', 'analyzing', 'ready', 'error'] as const) {
      expect(ProjectStatusSchema.parse(status)).toBe(status);
    }
  });

  it('rejects unknown status strings', () => {
    expect(ProjectStatusSchema.safeParse('queued').success).toBe(false);
    expect(ProjectStatusSchema.safeParse('').success).toBe(false);
    expect(ProjectStatusSchema.safeParse('READY').success).toBe(false);
  });
});

describe('LanguageBreakdownSchema', () => {
  it('accepts a valid language map', () => {
    const parsed = LanguageBreakdownSchema.parse({
      typescript: 12,
      rust: 3,
      python: 7,
    });
    expect(parsed.typescript).toBe(12);
    expect(parsed.rust).toBe(3);
  });

  it('rejects negative file counts', () => {
    expect(
      LanguageBreakdownSchema.safeParse({ typescript: -1 }).success,
    ).toBe(false);
  });

  it('rejects non-integer values', () => {
    expect(
      LanguageBreakdownSchema.safeParse({ typescript: 3.5 }).success,
    ).toBe(false);
  });
});

describe('ProjectSchema', () => {
  it('accepts a minimal valid project payload', () => {
    const parsed = ProjectSchema.parse({
      id: UUID_A,
      name: 'demo',
      rootPath: '/tmp/demo',
      fileCount: 4,
      totalSizeBytes: 1024,
      status: 'ready',
      languageBreakdown: { typescript: 4 },
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expect(parsed.name).toBe('demo');
    expect(parsed.status).toBe('ready');
  });

  it('rejects an empty name', () => {
    expect(
      ProjectSchema.safeParse({
        id: UUID_A,
        name: '',
        rootPath: '/tmp/demo',
        fileCount: 4,
        totalSizeBytes: 1024,
        status: 'ready',
        languageBreakdown: {},
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });

  it('rejects an invalid UUID', () => {
    expect(
      ProjectSchema.safeParse({
        id: 'not-a-uuid',
        name: 'demo',
        rootPath: '/tmp/demo',
        fileCount: 4,
        totalSizeBytes: 1024,
        status: 'ready',
        languageBreakdown: {},
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });

  it('rejects a negative fileCount', () => {
    expect(
      ProjectSchema.safeParse({
        id: UUID_A,
        name: 'demo',
        rootPath: '/tmp/demo',
        fileCount: -1,
        totalSizeBytes: 1024,
        status: 'ready',
        languageBreakdown: {},
        createdAt: ISO_TIMESTAMP,
        updatedAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });

  it('rejects an invalid timestamp', () => {
    expect(
      ProjectSchema.safeParse({
        id: UUID_A,
        name: 'demo',
        rootPath: '/tmp/demo',
        fileCount: 4,
        totalSizeBytes: 1024,
        status: 'ready',
        languageBreakdown: {},
        createdAt: 'not-a-date',
        updatedAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });
});
