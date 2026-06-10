import { mergeConfig } from 'vite';
import { configDefaults, defineConfig } from 'vitest/config';

import viteConfig from './vite.config';

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      environment: 'node',
      passWithNoTests: true,
      include: ['src/**/*.{test,spec}.{ts,tsx}'],
      exclude: [...configDefaults.exclude, 'src-tauri/**', 'src/**/*.integration.test.ts'],
      coverage: {
        // v8 provider — no Babel instrumentation, fast, ships with the
        // @vitest/coverage-v8 devDep. `text` for the CI log summary,
        // `lcov` for the uploaded artifact / external tooling (Codecov).
        provider: 'v8',
        reporter: ['text', 'lcov'],
        reportsDirectory: './coverage',
        // Mirror the rulebook's 80% target (rules.md §6): services +
        // utilities are the measured surface; UI components are exempt,
        // and integration/e2e specs never run under this config.
        include: ['src/lib/**/*.ts', 'src/stores/**/*.ts'],
        exclude: [
          ...configDefaults.coverage.exclude ?? [],
          'src/**/*.{test,spec}.{ts,tsx}',
          'src/**/*.integration.test.ts',
        ],
      },
    },
  }),
);
