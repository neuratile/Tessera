# @testing-ide/eslint-config

Shared ESLint presets for [Tessera](../../README.md).

| Export | Format |
|---|---|
| `@testing-ide/eslint-config/flat/base` | TypeScript flat-config factory |
| `@testing-ide/eslint-config/flat/react` | React flat-config factory |
| `@testing-ide/eslint-config/base`, `/react` | Legacy CommonJS configuration |

For an ESM `eslint.config.js`:

```js
import createReactConfig from "@testing-ide/eslint-config/flat/react";

export default createReactConfig({ tsconfigRootDir: import.meta.dirname });
```

The flat base combines ESLint recommendations and type-aware TypeScript rules.
React adds hooks and Fast Refresh rules. These presets do not install an
accessibility or import-order plugin. Check each workspace config for its actual
rules and install dependencies imported by the preset.
