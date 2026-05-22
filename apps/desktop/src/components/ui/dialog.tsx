import { useEffect, useRef, type ReactNode } from 'react';

/**
 * Lightweight side-sheet / drawer dialog primitive.
 *
 * Built in-house instead of pulling `@radix-ui/react-dialog` so we
 * stay light on deps for v0.1. Covers the three behaviours both the
 * Settings sheet and the Artifact-detail drawer were re-implementing
 * by hand:
 *
 *   1. Click backdrop → `onClose`.
 *   2. Press Escape   → `onClose`.
 *   3. Initial focus moves into the dialog when it opens, and the
 *      previously-focused element is restored on close.
 *
 * Focus trapping (Tab cycling inside the dialog) is intentionally
 * **not** implemented here — the side-drawer use case has the user
 * tab through a small linear stack of controls and reach the editor
 * panel behind the backdrop, where they cannot interact anyway
 * thanks to the backdrop's pointer-events. If a future flow needs
 * hard focus containment, swap to `@radix-ui/react-dialog` — the
 * prop surface here is intentionally a subset of theirs so the swap
 * is mechanical.
 *
 * `aria-labelledby` is wired automatically: `<DialogHeader title>`
 * generates a stable id and the surrounding `role="dialog"` host
 * references it. Custom headers can pass an `id` directly via
 * `labelledBy`.
 */

type DialogProps = {
  open: boolean;
  onClose: () => void;
  /**
   * Optional aria-labelledby id when the caller renders its own
   * heading. Falls back to a generated id used by `<DialogHeader>`.
   */
  labelledBy?: string;
  /**
   * Side the panel slides in from. `right` matches every existing
   * Tessera surface (settings sheet, artifact drawer); `left` is
   * reserved for future use.
   */
  side?: 'right' | 'left';
  /** Tailwind width class for the panel — default `max-w-md`. */
  widthClass?: string;
  /** Aria label fallback when there is no heading element. */
  ariaLabel?: string;
  children: ReactNode;
};

export function Dialog({
  open,
  onClose,
  labelledBy,
  side = 'right',
  widthClass = 'max-w-md',
  ariaLabel,
  children,
}: DialogProps) {
  const dialogRef = useRef<HTMLDivElement | null>(null);

  // Restore focus to whatever the user had focused before the
  // dialog opened. Avoids a confusing keyboard state after closing.
  useEffect(() => {
    if (!open) return;
    const active = document.activeElement;
    const previouslyFocused = active instanceof HTMLElement ? active : null;
    // Move focus into the dialog on mount so screen readers
    // announce its content immediately. We don't auto-focus the
    // first input on purpose — the user may have opened the dialog
    // via keyboard and wants the focus to land on the close /
    // cancel control next to where they came from.
    dialogRef.current?.focus();
    return () => {
      previouslyFocused?.focus();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', handler);
    return () => {
      window.removeEventListener('keydown', handler);
    };
  }, [open, onClose]);

  if (!open) return null;

  const sideClass = side === 'right' ? 'right-0 border-l' : 'left-0 border-r';

  return (
    <>
      <div
        className="bg-background/80 fixed inset-0 z-40 backdrop-blur-sm"
        onClick={onClose}
        aria-hidden="true"
      />
      <aside
        ref={dialogRef}
        tabIndex={-1}
        className={`fixed inset-y-0 ${sideClass} z-50 flex w-full ${widthClass} flex-col bg-card border-border shadow-2xl focus:outline-none`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        aria-label={labelledBy === undefined ? ariaLabel : undefined}
      >
        {children}
      </aside>
    </>
  );
}

// `useDialogTitleId` lives in `src/lib/dialog-title.ts` so this file
// exports a single React component and stays HMR-friendly.
