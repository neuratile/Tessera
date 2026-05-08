import { afterEach, describe, expect, it, vi } from 'vitest';

const { saveMock, writeTextFileMock } = vi.hoisted(() => ({
  saveMock: vi.fn(),
  writeTextFileMock: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: saveMock,
}));

vi.mock('@tauri-apps/plugin-fs', () => ({
  writeTextFile: writeTextFileMock,
}));

import { buildMarkdownFilename, exportMarkdownDocument } from './export-markdown';

afterEach(() => {
  saveMock.mockReset();
  writeTextFileMock.mockReset();
});

describe('buildMarkdownFilename', () => {
  it('should derive a stable markdown filename from the title', () => {
    expect(buildMarkdownFilename('Test Plan - Express API')).toBe('test-plan-express-api.md');
  });

  it('should fall back when the title has no slug characters', () => {
    expect(buildMarkdownFilename('***')).toBe('artifact.md');
  });
});

describe('exportMarkdownDocument', () => {
  it('should return null when the user cancels the save dialog', async () => {
    saveMock.mockResolvedValue(null);

    await expect(exportMarkdownDocument('Plan', '# Hello')).resolves.toBeNull();
    expect(writeTextFileMock).not.toHaveBeenCalled();
  });

  it('should write the markdown content to the chosen path', async () => {
    saveMock.mockResolvedValue('C:/tmp/test-plan.md');
    writeTextFileMock.mockResolvedValue(undefined);

    await expect(exportMarkdownDocument('Plan', '# Hello')).resolves.toBe('C:/tmp/test-plan.md');
    expect(writeTextFileMock).toHaveBeenCalledWith('C:/tmp/test-plan.md', '# Hello');
  });
});
