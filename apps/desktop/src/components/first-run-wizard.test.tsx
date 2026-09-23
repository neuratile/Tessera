// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  healthCheck: vi.fn(),
  listOllamaModels: vi.fn(),
  saveProviderConfig: vi.fn(),
  markOnboardingComplete: vi.fn(),
}));
vi.mock('@/lib/ipc', () => ({
  health: { healthCheck: mocks.healthCheck },
  providers: { listOllamaModels: mocks.listOllamaModels, saveProviderConfig: mocks.saveProviderConfig },
  getErrorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
}));
vi.mock('@/lib/onboarding', () => ({ markOnboardingComplete: mocks.markOnboardingComplete }));
import { FirstRunWizard } from './first-run-wizard';

beforeEach(() => {
  vi.clearAllMocks();
  mocks.healthCheck.mockRejectedValue(new Error('not connected'));
  mocks.listOllamaModels.mockResolvedValue([]);
  mocks.saveProviderConfig.mockResolvedValue('id');
});
afterEach(cleanup);

function goToModel() {
  fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
  fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
  fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
}

describe('FirstRunWizard completion', () => {
  it('clearly offers completion without AI when a model has not been saved', async () => {
    const onComplete = vi.fn();
    render(<FirstRunWizard onComplete={onComplete} />);
    goToModel();
    fireEvent.click(await screen.findByRole('button', { name: 'Continue without AI' }));
    expect(mocks.saveProviderConfig).not.toHaveBeenCalled();
    expect(mocks.markOnboardingComplete).toHaveBeenCalledTimes(1);
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it('only says Start using Tessera after saving the chosen model', async () => {
    const onComplete = vi.fn();
    render(<FirstRunWizard onComplete={onComplete} />);
    goToModel();
    fireEvent.click(screen.getByRole('button', { name: 'Use this model' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Start using Tessera' })).not.toBeNull());
    expect(mocks.saveProviderConfig).toHaveBeenCalledWith(expect.objectContaining({ defaultModel: 'qwen2.5-coder:7b', isActive: true }));
    fireEvent.click(screen.getByRole('button', { name: 'Start using Tessera' }));
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it('retains the saved model when stepping back and forward', async () => {
    render(<FirstRunWizard onComplete={vi.fn()} />);
    goToModel();
    fireEvent.click(screen.getByRole('radio', { name: /Light/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Use this model' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Start using Tessera' })).not.toBeNull());
    fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
    expect(await screen.findByRole('button', { name: 'Start using Tessera' })).not.toBeNull();
    expect(screen.getByText(/All set — qwen2.5-coder:1.5b/)).not.toBeNull();
  });

  it('shows save failures and never claims the unsaved model is ready', async () => {
    mocks.saveProviderConfig.mockRejectedValue(new Error('disk error'));
    render(<FirstRunWizard onComplete={vi.fn()} />);
    goToModel();
    fireEvent.click(screen.getByRole('button', { name: 'Use this model' }));
    expect((await screen.findByText('disk error')).textContent).toContain('disk error');
    expect(screen.getByRole('button', { name: 'Continue without AI' })).not.toBeNull();
    expect(mocks.markOnboardingComplete).not.toHaveBeenCalled();
  });
});
