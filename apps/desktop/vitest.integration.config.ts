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
    testTimeout: 600_000,
    hookTimeout: 600_000,
  },
});
