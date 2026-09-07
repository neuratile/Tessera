# @testing-ide/shared

Zod runtime validation and inferred TypeScript types for [Tessera](../../README.md).

Rust serde DTOs define backend wire behavior. Keep schemas aligned with actual
serialization and validate IPC payloads at the renderer boundary. Frontend-only
forms also use Zod; not every schema is a Rust DTO.

Import public schemas/types from the package root. Definitions live in
`src/schemas`, grouped re-exports in `src/types`, and `src/index.ts` exposes both
schema and type modules. Inspect the public entry before adding exports.

```bash
pnpm --filter @testing-ide/shared typecheck
pnpm --filter @testing-ide/shared test
```

Add malformed-input, optional-field, enum/casing, and model-specific tests when
changing contracts. Planned review DTOs follow the
[staged-review contract](../../plan/versions/v2/AI_FIRST_REVIEW.md).
