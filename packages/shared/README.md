# @testing-ide/shared

Shared **Zod-first** API contracts and inferred TypeScript types for the Testing IDE monorepo.

## Usage

Import from the package root:

```ts
import { UserSchema, type User, RegisterSchema } from '@testing-ide/shared';
```

Schemas live in `src/schemas/`; `src/types/` re-exports the same symbols for grouped imports. The package entry (`src/index.ts`) exports the public surface from `src/types/*` only (no duplicate exports).

## Scripts

- `npm run typecheck` — `tsc --noEmit`
- `npm test` — Vitest contract tests

When using pnpm at the repo root, prefer `pnpm --filter @testing-ide/shared typecheck`.
