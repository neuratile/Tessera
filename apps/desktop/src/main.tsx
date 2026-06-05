import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from '@/App';
import { AppErrorBoundary } from '@/components/app-error-boundary';
import { BrowserNotice } from '@/components/browser-notice';
import '@/index.css';
import { installE2eTauriMocks } from '@/lib/e2e-tauri-mocks';
import { initSentry } from '@/lib/sentry';
// Side-effect import - wires Monaco's web-worker URLs before any
// `<Editor>` mounts. See `lib/monaco-setup.ts`.
import '@/lib/monaco-setup';

const container = document.getElementById('root');
if (!container) {
  throw new Error('Root element #root not found');
}

/**
 * True when the page is rendered inside a real Tauri window (or a
 * Tauri E2E test harness that installs the mock IPC bridge).
 *
 * Tauri 2 exposes `__TAURI_INTERNALS__` on `window`; Tauri 1 used
 * `__TAURI__`. We probe both to stay future-proof if a downgrade
 * happens during a backport. The E2E test harness sets
 * `__TESTING_IDE_E2E__` before `installE2eTauriMocks` runs and we
 * accept that as a valid runtime too — otherwise the Playwright
 * suite would land on the browser splash.
 */
function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') return false;
  const hasOwn = (key: string): boolean =>
    Reflect.get(window, key) !== undefined;
  return (
    hasOwn('__TAURI_INTERNALS__') ||
    hasOwn('__TAURI__') ||
    hasOwn('__TESTING_IDE_E2E__')
  );
}

function bootstrap(rootElement: HTMLElement): void {
  installE2eTauriMocks();

  if (!isTauriRuntime()) {
    // Standalone Vite dev / preview build opened in a browser. Render
    // the splash instead of `<App />` so the user sees the desktop-app
    // instructions instead of a blank page full of failing IPC calls.
    createRoot(rootElement).render(
      <StrictMode>
        <BrowserNotice />
      </StrictMode>,
    );
    return;
  }

  initSentry();

  createRoot(rootElement).render(
    <StrictMode>
      <AppErrorBoundary>
        <App />
      </AppErrorBoundary>
    </StrictMode>,
  );
}

bootstrap(container);
