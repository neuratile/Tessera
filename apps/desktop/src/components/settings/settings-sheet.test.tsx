// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ listProviderConfigs: vi.fn(), deleteProviderConfig: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  providers: { listProviderConfigs: mocks.listProviderConfigs, deleteProviderConfig: mocks.deleteProviderConfig },
  ollama: { checkOllamaStatus: vi.fn().mockResolvedValue({ running: false, models: [] }) },
  getErrorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
}));
vi.mock('@/components/settings/embedding-config-panel', () => ({ EmbeddingConfigPanel: () => null }));
vi.mock('@/components/settings/jira-config-panel', () => ({ JiraConfigPanel: () => null }));
import { SettingsSheet } from './settings-sheet';
import { useUiStore } from '@/stores/ui-store';

beforeEach(() => {
  vi.clearAllMocks();
  useUiStore.getState().setSettingsOpen(true);
  mocks.listProviderConfigs.mockResolvedValue([{
    id: 'active', provider: 'ollama', isActive: true, defaultModel: 'test', baseUrl: null, hasApiKey: false,
  }]);
  mocks.deleteProviderConfig.mockResolvedValue(undefined);
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); useUiStore.getState().setSettingsOpen(false); });

describe('SettingsSheet deletion', () => {
  it('warns about deleting the active connection and respects cancel', async () => {
    const confirm = vi.fn().mockReturnValue(false);
    vi.stubGlobal('confirm', confirm);
    render(<SettingsSheet />);
    fireEvent.click(await screen.findByRole('button', { name: 'Delete ollama' }));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('active connection'));
    expect(mocks.deleteProviderConfig).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    fireEvent.click(screen.getByRole('button', { name: 'Delete ollama' }));
    await waitFor(() => expect(mocks.deleteProviderConfig).toHaveBeenCalledWith('active'));
  });
});
