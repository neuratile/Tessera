// @vitest-environment jsdom
import type { ConnectionTestResult, ProviderConfigView } from '@testing-ide/shared';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Hoisted so the `vi.mock` factory (which is itself hoisted above the
// imports) can reference these stubs without a TDZ error.
const mocks = vi.hoisted(() => ({
  listProviderConfigs: vi.fn(),
  saveProviderConfig: vi.fn(),
  deleteProviderConfig: vi.fn(),
  testProviderConnection: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  providers: {
    listProviderConfigs: mocks.listProviderConfigs,
    saveProviderConfig: mocks.saveProviderConfig,
    deleteProviderConfig: mocks.deleteProviderConfig,
    testProviderConnection: mocks.testProviderConnection,
  },
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : String(error),
}));

import { ProviderConfigPanel } from './provider-config-panel';

const savedConfig: ProviderConfigView = {
  id: '11111111-1111-1111-1111-111111111111',
  provider: 'openai',
  hasApiKey: true,
  baseUrl: null,
  defaultModel: 'gpt-4o-mini',
  isActive: true,
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listProviderConfigs.mockResolvedValue([]);
});

afterEach(cleanup);

describe('ProviderConfigPanel', () => {
  it('shows the empty state once the config list loads', async () => {
    render(<ProviderConfigPanel />);
    expect(await screen.findByText('No provider configs saved yet.')).not.toBeNull();
    expect(mocks.listProviderConfigs).toHaveBeenCalledTimes(1);
  });

  it('renders a saved config returned from IPC', async () => {
    mocks.listProviderConfigs.mockResolvedValue([savedConfig]);
    render(<ProviderConfigPanel />);
    expect(await screen.findByText('openai')).not.toBeNull();
    expect(screen.getByText(/Model: gpt-4o-mini/)).not.toBeNull();
    expect(screen.getByText(/Key: saved/)).not.toBeNull();
  });

  it('switches the API-key field to "leave blank to keep key" when editing a config with a stored key', async () => {
    mocks.listProviderConfigs.mockResolvedValue([savedConfig]);
    render(<ProviderConfigPanel />);
    await screen.findByText('openai');

    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));

    expect(
      screen.getByPlaceholderText('Leave blank to keep the saved key'),
    ).not.toBeNull();
    expect(screen.getByText('Clear the saved key on the next save')).not.toBeNull();
  });

  it('saves the current form through IPC and surfaces the result', async () => {
    mocks.saveProviderConfig.mockResolvedValue('new-id');
    render(<ProviderConfigPanel />);
    await screen.findByText('No provider configs saved yet.');

    fireEvent.click(screen.getByRole('button', { name: 'Save config' }));

    await waitFor(() => {
      expect(mocks.saveProviderConfig).toHaveBeenCalledTimes(1);
    });
    // Blank API key is sent as `undefined` (preserve any stored key);
    // the default provider/model from the empty form flow through.
    expect(mocks.saveProviderConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: 'ollama',
        defaultModel: 'qwen2.5-coder:7b',
        isActive: true,
      }),
    );
    expect(await screen.findByText('Saved config for ollama.')).not.toBeNull();
  });

  it('runs a connection test and renders latency + status', async () => {
    const result: ConnectionTestResult = {
      ok: true,
      latencyMs: 42,
      message: 'pong',
      models: ['m-one', 'm-two'],
    };
    mocks.testProviderConnection.mockResolvedValue(result);
    render(<ProviderConfigPanel />);
    await screen.findByText('No provider configs saved yet.');

    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }));

    expect(await screen.findByText('Latency: 42 ms')).not.toBeNull();
    expect(screen.getByText('pong')).not.toBeNull();
    expect(mocks.testProviderConnection).toHaveBeenCalledTimes(1);
  });
});
