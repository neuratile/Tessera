/// <reference types="vitest" />
import { readFileSync } from 'node:fs';
import path from 'node:path';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { z } from 'zod';

const PackageJsonSchema = z.object({
  version: z.string().trim().min(1),
});

function readPackageVersion(): string {
  const parsed = PackageJsonSchema.safeParse(
    JSON.parse(readFileSync(path.resolve(__dirname, './package.json'), 'utf8')),
  );
  return parsed.success ? parsed.data.version : '0.0.0-dev';
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    __APP_VERSION__: JSON.stringify(readPackageVersion()),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'node',
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
  },
});
