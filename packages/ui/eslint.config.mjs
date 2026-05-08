import createTypeScriptConfig from '../eslint-config/flat/base.mjs';

export default createTypeScriptConfig({
  tsconfigRootDir: import.meta.dirname,
  ignores: ['eslint.config.mjs'],
});
