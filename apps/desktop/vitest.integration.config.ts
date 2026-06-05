import path from 'node:path';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'node',
    passWithNoTests: true,
    include: ['src/**/*.integration.test.ts', 'tests/**/*.integration.test.ts'],
    exclude: ['src-tauri/**'],
    fileParallelism: false,
    // Must stay above PROCESS_TIMEOUT_MS in tests/support/ollama.ts
    // (15 min) so the cargo probe's own timeout fires first and reports
    // a meaningful error instead of a bare vitest timeout.
    testTimeout: 960_000,
    hookTimeout: 960_000,
  },
});
