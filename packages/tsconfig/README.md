# @testing-ide/tsconfig

Shared TypeScript configuration presets for the [Tessera](../../README.md) monorepo.

## Presets

| Entry | Use for |
|-------|---------|
| `@testing-ide/tsconfig/base` | Pure TypeScript / Node packages — strict mode, ES2022 target, isolated modules |
| `@testing-ide/tsconfig/desktop` | Desktop app (`apps/desktop`) — adds DOM + `vite/client` types and JSX runtime |

## Usage

In a consuming workspace `tsconfig.json`:

```json
{
  "extends": "@testing-ide/tsconfig/desktop",
  "include": ["src", "vite.config.ts"]
}
```

The presets share compiler options; they do not both set `noEmit`. Workspace
`typecheck` scripts invoke `tsc --noEmit` explicitly. Production frontend output
is built by Vite and packaged with Tauri.
