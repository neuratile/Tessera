# Repository context for coding agents

Read [AGENTS](./AGENTS.md) and [Agent workflow](./docs/AGENT_WORKFLOW.md).
Current product facts live in [Project status](./docs/PROJECT_STATUS.md).

Tessera generates artifacts using AST/retrieval and a selected LLM, and optionally
executes JS/TS or Python tests in a hardened Docker sandbox. Cloud model and
embedding selections send relevant context off-device.

Staged “Review my changes” is planned. Its
[contract](./plan/versions/v2/AI_FIRST_REVIEW.md) and
[fixture](./evals/README.md) are available; runtime, CLI, and MCP are not shipped.

## Implementation rules

- Renderer: `apps/desktop/src`; backend: `apps/desktop/src-tauri/src`.
- Commands validate/delegate; services orchestrate; repositories own SQL;
  providers isolate external payloads; prompts own prompt logic.
- Match Zod schemas to Rust serde wire DTOs; test malformed inputs, casing,
  optional fields, and provider/model-specific payloads.
- Preserve the singleton active LLM. Embedding selection is independent.
- SQLite uses embedding BLOBs and cosine scans; vec0 remains a future migration.
- Preserve sandbox opt-in, network isolation, limits, safe paths, and cancellation.
- Healing generated tests does not prove application repair; planned review
  evidence binds immutable source and test versions.
- Never commit keys/environment files or put backend secrets in `VITE_*`.

See [Architecture](./docs/ARCHITECTURE.md) and [rules](./rules/rules.md).

## Verify and deliver

```bash
pnpm install --frozen-lockfile
pnpm --filter @testing-ide/desktop dev
pnpm test:eval-fixtures
pnpm guard:pre-push
```

[Getting started](./docs/GETTING_STARTED.md) covers environment files and Windows
shell setup. [CI/CD](./docs/CI_CD.md) explains command coverage.

Use one task per branch/worktree, inspect the diff, run checks, and address review
feedback. Never bypass failing guards or push directly to master. Preserve six
required CI names. Do not merge, tag, publish, or enable auto-merge without user
authorization. [Release automation](./docs/RELEASING.md) produces checked drafts;
it does not publish automatically or configure signing.
