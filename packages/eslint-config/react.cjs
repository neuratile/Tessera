const base = require('./base.cjs');

/** @type {import('eslint').Linter.Config} */
module.exports = {
  ...base,
  env: {
    ...base.env,
    browser: true,
  },
  plugins: [...(base.plugins ?? []), 'react-hooks', 'react-refresh'],
  extends: [...(base.extends ?? []), 'plugin:react-hooks/recommended'],
  rules: {
    ...base.rules,
    'react-refresh/only-export-components': 'warn',
  },
};
