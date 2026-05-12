import { AlertTriangle, Check, Info, X, XCircle } from 'lucide-react';
import { useEffect, type ReactElement } from 'react';

import { useToastStore, type Toast, type ToastKind } from '@/stores/toast-store';

/**
 * Toast stack renderer.
 *
 * Mounted once in `App.tsx`. Subscribes to `useToastStore` and pins
 * a column of toasts to the bottom-right corner above the status
 * bar. Each toast auto-dismisses after its own `duration` via a
 * dedicated `useEffect` timer per row — store-level timers would
 * complicate testing and couple business state to wall-clock.
 *
 * Polite ARIA live region — screen readers announce new toasts but
 * are not interrupted mid-sentence.
 */
export function ToastViewport() {
  const toasts = useToastStore((s) => s.toasts);

  if (toasts.length === 0) return null;

  return (
    <div
      role="region"
      aria-label="Notifications"
      className="pointer-events-none fixed bottom-10 right-3 z-50 flex w-[320px] flex-col gap-2"
    >
      {toasts.map((t) => (
        <ToastRow key={t.id} toast={t} />
      ))}
    </div>
  );
}

function ToastRow({ toast }: { toast: Toast }) {
  const dismiss = useToastStore((s) => s.dismiss);

  useEffect(() => {
    if (toast.duration <= 0) return;
    const handle = window.setTimeout(() => dismiss(toast.id), toast.duration);
    return () => window.clearTimeout(handle);
  }, [toast.id, toast.duration, dismiss]);

  const palette = KIND_PALETTE[toast.kind];

  return (
    <div
      role="status"
      aria-live="polite"
      className={`pointer-events-auto flex items-start gap-2 rounded-md border bg-card p-3 shadow-lg ${palette.frame}`}
    >
      <span className={`mt-0.5 ${palette.icon}`}>{palette.glyph}</span>
      <div className="min-w-0 flex-1">
        {toast.title !== undefined ? (
          <p className={`text-[10px] font-semibold uppercase tracking-[0.12em] ${palette.title}`}>
            {toast.title}
          </p>
        ) : null}
        <p className="text-xs text-foreground" title={toast.message}>
          {toast.message}
        </p>
      </div>
      <button
        type="button"
        aria-label="Dismiss notification"
        onClick={() => dismiss(toast.id)}
        className="text-muted-foreground hover:bg-muted hover:text-foreground rounded p-0.5"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

type Palette = {
  frame: string;
  icon: string;
  title: string;
  glyph: ReactElement;
};

const KIND_PALETTE: Record<ToastKind, Palette> = {
  ok: {
    frame: 'border-success/30',
    icon: 'text-success',
    title: 'text-success',
    glyph: <Check className="size-3.5" />,
  },
  err: {
    frame: 'border-destructive/30',
    icon: 'text-destructive',
    title: 'text-destructive',
    glyph: <XCircle className="size-3.5" />,
  },
  warning: {
    frame: 'border-warning/30',
    icon: 'text-warning',
    title: 'text-warning',
    glyph: <AlertTriangle className="size-3.5" />,
  },
  info: {
    frame: 'border-border',
    icon: 'text-muted-foreground',
    title: 'text-muted-foreground',
    glyph: <Info className="size-3.5" />,
  },
};
