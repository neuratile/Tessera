// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { Dialog } from './dialog';

/**
 * Behaviour contract for the in-house side-sheet primitive: it renders
 * its children only when open, exposes the `role="dialog"` host with the
 * right aria wiring, and closes on both the Escape key (window-level
 * listener) and a backdrop click. These are the three behaviours the
 * Settings sheet + artifact drawer rely on, so they are worth locking.
 */
afterEach(cleanup);

describe('Dialog', () => {
  it('renders nothing while closed', () => {
    render(
      <Dialog open={false} onClose={vi.fn()}>
        <p>panel body</p>
      </Dialog>,
    );
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.queryByText('panel body')).toBeNull();
  });

  it('renders children in a modal dialog with the supplied aria-label', () => {
    render(
      <Dialog open onClose={vi.fn()} ariaLabel="Settings">
        <p>panel body</p>
      </Dialog>,
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-label')).toBe('Settings');
    expect(screen.getByText('panel body')).not.toBeNull();
  });

  it('prefers aria-labelledby over aria-label when given', () => {
    render(
      <Dialog open onClose={vi.fn()} labelledBy="title-1" ariaLabel="ignored">
        <h2 id="title-1">Heading</h2>
      </Dialog>,
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-labelledby')).toBe('title-1');
    expect(dialog.getAttribute('aria-label')).toBeNull();
  });

  it('calls onClose when Escape is pressed', () => {
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose}>
        <p>body</p>
      </Dialog>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('calls onClose when the backdrop is clicked', () => {
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose}>
        <p>body</p>
      </Dialog>,
    );
    const backdrop = document.querySelector('[aria-hidden="true"]');
    expect(backdrop).not.toBeNull();
    if (backdrop !== null) {
      fireEvent.click(backdrop);
    }
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('cycles Tab within the sheet and restores focus on close', () => {
    const { rerender } = render(
      <>
        <button type="button">Opener</button>
        <Dialog open={false} onClose={vi.fn()} ariaLabel="Settings">
          <button type="button">First</button>
          <button type="button">Last</button>
        </Dialog>
      </>,
    );
    screen.getByRole('button', { name: 'Opener' }).focus();
    rerender(
      <>
        <button type="button">Opener</button>
        <Dialog open onClose={vi.fn()} ariaLabel="Settings">
          <button type="button">First</button>
          <button type="button">Last</button>
        </Dialog>
      </>,
    );
    const first = screen.getByRole('button', { name: 'First' });
    const last = screen.getByRole('button', { name: 'Last' });
    last.focus();
    fireEvent.keyDown(window, { key: 'Tab' });
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(window, { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(last);
    screen.getByRole('button', { name: 'Opener' }).focus();
    expect(document.activeElement).toBe(screen.getByRole('dialog'));
    rerender(<button type="button">Opener</button>);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Opener' }));
  });

  it('keeps focus on the sheet when no controls exist', () => {
    render(<Dialog open onClose={vi.fn()} ariaLabel="Empty"><p>No controls</p></Dialog>);
    fireEvent.keyDown(window, { key: 'Tab' });
    expect(document.activeElement).toBe(screen.getByRole('dialog'));
  });

  it('only closes the topmost nested dialog on Escape', () => {
    const outer = vi.fn();
    const inner = vi.fn();
    render(<Dialog open onClose={outer} ariaLabel="Outer">
      <Dialog open onClose={inner} ariaLabel="Inner"><button type="button">Close</button></Dialog>
    </Dialog>);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });

  it('does not wire the Escape listener while closed', () => {
    const onClose = vi.fn();
    render(
      <Dialog open={false} onClose={onClose}>
        <p>body</p>
      </Dialog>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });
});
