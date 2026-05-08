import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';

export default function createTypeScriptConfig({
  tsconfigRootDir,
  ignores = [],
} = {}) {
  return tseslint.config(
    {
      ignores: ['dist', 'node_modules', ...ignores],
    },
    eslint.configs.recommended,
    ...tseslint.configs.recommendedTypeChecked,
    {
      languageOptions: {
        parserOptions: {
          projectService: true,
          tsconfigRootDir,
        },
      },
    },
    {
      rules: {
        '@typescript-eslint/no-explicit-any': 'error',
        '@typescript-eslint/no-floating-promises': 'error',
        '@typescript-eslint/consistent-type-definitions': ['error', 'type'],
      },
    },
  );
}
