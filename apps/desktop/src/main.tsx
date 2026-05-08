import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import './index.css';
import { installE2eTauriMocks } from './lib/e2e-tauri-mocks';
import { initSentry } from './lib/sentry';
// Side-effect import - wires Monaco's web-worker URLs before any
// `<Editor>` mounts. See `lib/monaco-setup.ts`.
import './lib/monaco-setup';

const container = document.getElementById('root');
if (!container) {
  throw new Error('Root element #root not found');
}

function bootstrap(rootElement: HTMLElement): void {
  installE2eTauriMocks();
  initSentry();

  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

bootstrap(container);
