import { afterEach, describe, expect, it, vi } from 'vitest';

const { saveMock, invokeMock } = vi.hoisted(() => ({
  saveMock: vi.fn(),
  invokeMock: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: saveMock,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}));

import { buildExportFilename, exportArtifactToFile } from './export-artifact';

afterEach(() => {
  saveMock.mockReset();
  invokeMock.mockReset();
});

describe('buildExportFilename', () => {
  it('slugs the title and appends the extension', () => {
    expect(buildExportFilename('Test Plan - Express API', 'xlsx')).toBe(
      'test-plan-express-api.xlsx',
    );
  });

  it('falls back when the title has no slug characters', () => {
    expect(buildExportFilename('***', 'csv')).toBe('artifact.csv');
    expect(buildExportFilename('   ', 'tsv')).toBe('artifact.tsv');
  });

  it('caps the slug at 80 characters', () => {
    const long = 'x'.repeat(200);
    const name = buildExportFilename(long, 'csv');
    expect(name).toBe(`${'x'.repeat(80)}.csv`);
  });
});

describe('exportArtifactToFile', () => {
  it('returns null without invoking the backend when the dialog is cancelled', async () => {
    saveMock.mockResolvedValue(null);

    await expect(exportArtifactToFile('a1', 'Plan', 'xlsx')).resolves.toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('invokes export_artifact with the chosen path and parses the outcome', async () => {
    saveMock.mockResolvedValue('C:/tmp/plan.xlsx');
    invokeMock.mockResolvedValue({ files: ['C:/tmp/plan.xlsx'] });

    const outcome = await exportArtifactToFile('a1', 'Plan', 'xlsx');
    expect(outcome).toEqual({ files: ['C:/tmp/plan.xlsx'] });
    expect(invokeMock).toHaveBeenCalledWith('export_artifact', {
      artifactId: 'a1',
      format: 'xlsx',
      destPath: 'C:/tmp/plan.xlsx',
    });
  });

  it('offers the format-specific dialog filter', async () => {
    saveMock.mockResolvedValue(null);

    await exportArtifactToFile('a1', 'My Cases', 'csv');
    expect(saveMock).toHaveBeenCalledWith({
      title: 'Export CSV',
      defaultPath: 'my-cases.csv',
      filters: [{ name: 'CSV', extensions: ['csv'] }],
    });
  });

  it('rejects when the backend reports a schema-invalid outcome', async () => {
    saveMock.mockResolvedValue('C:/tmp/plan.csv');
    invokeMock.mockResolvedValue({ files: [] });

    await expect(exportArtifactToFile('a1', 'Plan', 'csv')).rejects.toThrow(
      /schema validation/u,
    );
  });
});
