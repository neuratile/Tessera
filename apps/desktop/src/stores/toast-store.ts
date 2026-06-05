import { create } from 'zustand';

/**
 * Global toast stack.
 *
 * Single source of truth for transient notifications. Previously the
 * toolbar's analyze toast was a component-local `useState`; that
 * pattern duplicated timer + kind + dismiss logic in every component
 * that wanted to fire a message. The store centralises it so any
 * call site (toolbar, AI panel, settings sheet, future command
 * palette) can push a toast without owning the rendering surface.
 *
 * Rendering lives in `components/ui/toast-viewport.tsx` and is
 * mounted once in `App.tsx`. The viewport listens to this store,
 * stacks toasts in the bottom-right corner, and auto-dismisses each
 * after its `duration` ms.
 */

export type ToastKind = 'ok' | 'err' | 'info' | 'warning';

export type Toast = {
  id: number;
  kind: ToastKind;
  message: string;
  /** Optional short label rendered above the message (e.g. "Analyze"). */
  title?: string;
  /** Auto-dismiss timeout in milliseconds. Defaults to 6000. */
  duration: number;
};

type PushArgs = {
  kind: ToastKind;
  message: string;
  title?: string;
  duration?: number;
};

type ToastState = {
  toasts: Toast[];
  push: (args: PushArgs) => number;
  dismiss: (id: number) => void;
  clear: () => void;
};

const DEFAULT_DURATION_MS = 6_000;

let nextId = 1;

const store = create<ToastState>()((set) => ({
  toasts: [],
  push: ({ kind, message, title, duration = DEFAULT_DURATION_MS }) => {
    const id = nextId++;
    set((state) => {
      const next: Toast =
        title === undefined
          ? { id, kind, message, duration }
          : { id, kind, message, duration, title };
      return { toasts: [...state.toasts, next] };
    });
    return id;
  },
  dismiss: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((t) => t.id !== id),
    })),
  clear: () => set({ toasts: [] }),
}));

const globalStore = globalThis as unknown as {
  useToastStore?: typeof store;
};

export const useToastStore = globalStore.useToastStore || store;

if (process.env.NODE_ENV !== 'production') {
  globalStore.useToastStore = useToastStore;
}


/**
 * Imperative API for code that doesn't sit inside a React component.
 * Reads the store snapshot rather than subscribing.
 */
export const toast = {
  ok: (message: string, opts?: Omit<PushArgs, 'kind' | 'message'>) =>
    useToastStore.getState().push({ kind: 'ok', message, ...opts }),
  err: (message: string, opts?: Omit<PushArgs, 'kind' | 'message'>) =>
    useToastStore.getState().push({ kind: 'err', message, ...opts }),
  info: (message: string, opts?: Omit<PushArgs, 'kind' | 'message'>) =>
    useToastStore.getState().push({ kind: 'info', message, ...opts }),
  warning: (message: string, opts?: Omit<PushArgs, 'kind' | 'message'>) =>
    useToastStore.getState().push({ kind: 'warning', message, ...opts }),
};
