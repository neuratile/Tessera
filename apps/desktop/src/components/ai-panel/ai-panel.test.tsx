// @vitest-environment jsdom
import type { ArtifactSummary, Project } from '@testing-ide/shared';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  listArtifacts: vi.fn(), approveArtifact: vi.fn(), rejectArtifact: vi.fn(), listExternalLinks: vi.fn(),
  bulkPushArtifactsToJira: vi.fn(), listProviderConfigs: vi.fn(),
}));
vi.mock('@/lib/ipc', () => ({
  artifacts: { listArtifacts: mocks.listArtifacts, approveArtifact: mocks.approveArtifact, rejectArtifact: mocks.rejectArtifact },
  trackers: { listExternalLinks: mocks.listExternalLinks, bulkPushArtifactsToJira: mocks.bulkPushArtifactsToJira },
  providers: { listProviderConfigs: mocks.listProviderConfigs },
  streaming: { subscribeToGenerationEvents: vi.fn().mockResolvedValue(() => {}) },
  generation: {}, filesystem: {},
  getErrorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
}));
vi.mock('@/components/ai-panel/artifact-detail-drawer', () => ({ ArtifactDetailDrawer: () => null }));
import { AiPanel } from './ai-panel';
import { useWorkspaceStore } from '@/stores/workspace-store';
import { useAiStore } from '@/stores/ai-store';

const project = (id: string) => ({ id, name: id }) as Project;
const artifact = (id: string) => ({
  id, title: id, artifactType: 'test-cases', model: 'test', status: 'draft', version: 1,
}) as ArtifactSummary;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => {
  vi.clearAllMocks();
  useAiStore.getState().setArtifacts([]);
  useWorkspaceStore.setState({ project: project('A') });
  mocks.listProviderConfigs.mockResolvedValue([]);
  mocks.listExternalLinks.mockResolvedValue([]);
  mocks.approveArtifact.mockResolvedValue(undefined);
  mocks.rejectArtifact.mockResolvedValue(undefined);
  mocks.bulkPushArtifactsToJira.mockResolvedValue([]);
});
afterEach(cleanup);

describe('AiPanel project-scoped queue', () => {
  it('clears selection on project switch and bulk actions only use the current project', async () => {
    mocks.listArtifacts.mockImplementation((id: string) => Promise.resolve([artifact(id)]));
    render(<AiPanel />);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Select A' }));
    expect(screen.getByText('1 selected')).not.toBeNull();
    act(() => useWorkspaceStore.setState({ project: project('B') }));
    expect(screen.queryByText('1 selected')).toBeNull();
    expect(await screen.findByRole('checkbox', { name: 'Select B' })).not.toBeNull();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Select B' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'Approve' })[0]!);
    await waitFor(() => expect(mocks.approveArtifact).toHaveBeenCalledWith('B'));
    expect(mocks.approveArtifact).not.toHaveBeenCalledWith('A');
    fireEvent.click(screen.getByRole('checkbox', { name: 'Select B' }));
    fireEvent.click(screen.getByRole('button', { name: 'Push to Jira' }));
    await waitFor(() => expect(mocks.bulkPushArtifactsToJira).toHaveBeenCalledWith(['B']));
  });

  it('keeps the current queue when a manual refresh fails', async () => {
    mocks.listArtifacts.mockResolvedValueOnce([artifact('A')]).mockRejectedValueOnce(new Error('offline'));
    render(<AiPanel />);
    expect(await screen.findByRole('checkbox', { name: 'Select A' })).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh review queue' }));
    await screen.findByText('offline');
    expect(screen.getByRole('checkbox', { name: 'Select A' })).not.toBeNull();
  });

  it('ignores an old bulk push after selecting a new project artifact', async () => {
    const old = deferred<never[]>();
    mocks.listArtifacts.mockImplementation((id: string) => Promise.resolve([artifact(id)]));
    mocks.bulkPushArtifactsToJira.mockReturnValue(old.promise);
    render(<AiPanel />);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Select A' }));
    fireEvent.click(screen.getByRole('button', { name: 'Push to Jira' }));
    act(() => useWorkspaceStore.setState({ project: project('B') }));
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Select B' }));
    await act(async () => { old.resolve([]); await old.promise; });
    expect(screen.getByText('1 selected')).not.toBeNull();
  });

  it('does not clear a new selection when an earlier bulk approval finishes', async () => {
    const old = deferred<void>();
    mocks.listArtifacts.mockImplementation((id: string) => Promise.resolve([artifact(id)]));
    mocks.approveArtifact.mockReturnValue(old.promise);
    render(<AiPanel />);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Select A' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'Approve' })[0]!);
    act(() => useWorkspaceStore.setState({ project: project('B') }));
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Select B' }));
    await act(async () => { old.resolve(); await old.promise; });
    expect(screen.getByText('1 selected')).not.toBeNull();
  });

  it('does not show an earlier project approval error in the new project', async () => {
    let fail!: (error: Error) => void;
    mocks.listArtifacts.mockImplementation((id: string) => Promise.resolve([artifact(id)]));
    mocks.approveArtifact.mockReturnValue(new Promise<void>((_, reject) => { fail = reject; }));
    render(<AiPanel />);
    await screen.findByRole('checkbox', { name: 'Select A' });
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }));
    act(() => useWorkspaceStore.setState({ project: project('B') }));
    await screen.findByRole('checkbox', { name: 'Select B' });
    await act(async () => { fail(new Error('A failed')); await Promise.resolve(); });
    expect(screen.queryByText('A failed')).toBeNull();
  });

  it('ignores an old project response after the new project queue has loaded', async () => {
    const old = deferred<ArtifactSummary[]>();
    mocks.listArtifacts.mockImplementation((id: string) => id === 'A' ? old.promise : Promise.resolve([artifact('B')]));
    render(<AiPanel />);
    await waitFor(() => expect(mocks.listArtifacts).toHaveBeenCalledWith('A'));
    act(() => useWorkspaceStore.setState({ project: project('B') }));
    expect(await screen.findByRole('checkbox', { name: 'Select B' })).not.toBeNull();
    await act(async () => { old.resolve([artifact('A')]); await old.promise; });
    expect(screen.queryByRole('checkbox', { name: 'Select A' })).toBeNull();
    expect(screen.getByRole('checkbox', { name: 'Select B' })).not.toBeNull();
  });
});
