import createReactConfig from '../../packages/eslint-config/flat/react.mjs';

export default createReactConfig({
  tsconfigRootDir: import.meta.dirname,
  ignores: [
    'eslint.config.mjs',
    'playwright-report',
    'src-tauri',
    'scripts/**',
    'test-results',
    'tests/golden/fixtures/**',
  ],
});
