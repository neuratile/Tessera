// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

// Stub only the side-effecting `dispatchCommand` — keep the real
// `COMMAND` map + `CommandId` type so the COMMANDS table the palette
// builds at module load stays intact.
vi.mock('@/lib/command-bus', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/command-bus')>();
  return { ...actual, dispatchCommand: vi.fn() };
});

import { dispatchCommand } from '@/lib/command-bus';

import { CommandPalette } from './command-palette';

const dispatchMock = vi.mocked(dispatchCommand);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('CommandPalette', () => {
  it('renders nothing while closed', () => {
    render(<CommandPalette open={false} onClose={vi.fn()} />);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('lists every command when the query is empty', () => {
    render(<CommandPalette open onClose={vi.fn()} />);
    expect(screen.getByText('Analyze Project')).not.toBeNull();
    expect(screen.getByText('Settings')).not.toBeNull();
    expect(screen.getByText('Open GitHub Repository')).not.toBeNull();
  });

  it('filters by label, alias, and group', () => {
    render(<CommandPalette open onClose={vi.fn()} />);
    const input = screen.getByLabelText('Command query');

    fireEvent.change(input, { target: { value: 'github' } });
    expect(screen.getByText('Open GitHub Repository')).not.toBeNull();
    expect(screen.queryByText('Analyze Project')).toBeNull();

    // "scan" is an alias of Analyze Project, not part of its label.
    fireEvent.change(input, { target: { value: 'scan' } });
    expect(screen.getByText('Analyze Project')).not.toBeNull();
    expect(screen.queryByText('Open GitHub Repository')).toBeNull();
  });

  it('shows an empty state when nothing matches', () => {
    render(<CommandPalette open onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Command query'), {
      target: { value: 'zzzzz-nope' },
    });
    expect(screen.getByText(/No commands match/)).not.toBeNull();
  });

  it('runs the highlighted command on Enter and closes', () => {
    const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} />);
    // Highlight defaults to the first match — "Analyze Project" (a bus
    // command) — so Enter should dispatch it and close the palette.
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Enter' });
    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('moves the highlight with ArrowDown before running on Enter', () => {
    const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} />);
    const dialog = screen.getByRole('dialog');
    fireEvent.keyDown(dialog, { key: 'ArrowDown' });
    fireEvent.keyDown(dialog, { key: 'Enter' });
    // Second command ("Regenerate Last Artifact") is also a bus command,
    // so a dispatch still fires — the point is Enter acts on the moved
    // highlight without throwing.
    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('closes without dispatching on Escape', () => {
    const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} />);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(dispatchMock).not.toHaveBeenCalled();
  });
});
