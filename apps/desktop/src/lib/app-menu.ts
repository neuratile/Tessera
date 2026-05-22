import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect } from 'react';

import { dispatchCommand, isCommandId } from './command-bus';
import { logToBackend } from './ipc/system';

/**
 * Bridge from the native menu bar to the renderer's command bus.
 *
 * `apps/desktop/src-tauri/src/menu.rs` emits an `app:menu` Tauri
 * event with the clicked item's stable id as the payload. This hook
 * subscribes once at app mount and re-fires the payload through
 * `dispatchCommand` so individual components (Toolbar, Settings
 * sheet, AI panel) can listen for just the commands they own.
 *
 * Mount-once semantics: the listener is installed on the first call
 * and detached on cleanup. Multiple components calling this hook is
 * safe — each installs its own listener — but the App shell is
 * intended to be the only caller in practice.
 */
export function useAppMenuEvents(): void {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;

    void listen<string>('app:menu', (event) => {
      const id = event.payload;
      if (isCommandId(id)) {
        dispatchCommand(id);
        return;
      }
      // Forward unknown ids to the Rust tracing subscriber so a rename
      // mismatch between menu.rs and command-bus.ts surfaces during
      // development without violating the "no console.log in frontend"
      // rule.
      void logToBackend(
        'warn',
        'app-menu',
        `unknown command id from native menu: ${String(id)}`,
      );
    })
      .then((u) => {
        if (cancelled) {
          u();
        } else {
          unlisten = u;
        }
      })
      .catch((err: unknown) => {
        // A failure here leaves the native menu wired but inert — every
        // click silently does nothing. Surface to Rust-side tracing so
        // the failure is at least visible in logs / Sentry.
        const message = err instanceof Error ? err.message : String(err);
        void logToBackend('error', 'app-menu', `listen('app:menu') failed: ${message}`);
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
