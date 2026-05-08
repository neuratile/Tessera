import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';

import createTypeScriptConfig from './base.mjs';

export default function createReactConfig(options = {}) {
  return [
    ...createTypeScriptConfig(options),
    {
      plugins: { 'react-hooks': reactHooks },
      rules: reactHooks.configs.recommended.rules,
    },
    {
      plugins: { 'react-refresh': reactRefresh },
      rules: { 'react-refresh/only-export-components': 'warn' },
    },
  ];
}
