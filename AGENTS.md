# Repository Guidelines

## Project Structure & Module Organization

Tessera is a local-first AI testing IDE built as a Tauri desktop app. The React/TypeScript renderer lives in `apps/desktop/src`, with UI components, stores, utilities, assets, and frontend tests colocated there. The Rust backend lives in `apps/desktop/src-tauri/src`; keep commands thin, put orchestration in `services`, data access in `repositories`, provider integrations in `providers`, and prompt logic in `prompts`. Shared schemas and types are in `packages/shared/src`; tooling presets live in `packages/eslint-config` and `packages/tsconfig`. Supporting material lives in `docs/`, `plan/`, `rules/`, and `tools/scripts/`.

The staged-review contract and checkout fixture are available; the review runtime, CLI, and MCP are planned. See [current status](docs/PROJECT_STATUS.md) and [documentation index](docs/README.md).

## Build, Test, and Development Commands

- `pnpm install`: install workspace dependencies using pnpm 10.
- `pnpm dev`: run the monorepo development targets through Turbo.
- `pnpm build`: build all workspace packages and apps.
- `pnpm lint`: run lint checks across the workspace.
- `pnpm typecheck`: run TypeScript type checks.
- `pnpm test`: run release tooling, review fixtures, and workspace tests.
- `pnpm test:eval-fixtures`: check the seeded checkout regression and clean control.
- `pnpm test:tooling`: check release tag/version validation.
- `pnpm --filter @testing-ide/desktop test:rust`: run Rust tests for the Tauri backend.
- `pnpm services:up` / `pnpm services:down`: start or stop local support services.
- `pnpm bootstrap:ollama`: prepare the default local Ollama setup.

## Coding Style & Naming Conventions

Use TypeScript, React, ESLint, and Vitest conventions in frontend code. Prefer PascalCase for React components, camelCase for functions and variables, and kebab-case for route or asset filenames when applicable. Rust code should be formatted with `rustfmt`, pass `clippy`, and use snake_case for modules, functions, and fields. Preserve the existing backend layering: commands validate and delegate, services coordinate behavior, repositories own SQL, and providers isolate external APIs.

The **active LLM connection is a singleton**: exactly one `user_provider_configs` row is `is_active` at a time, enforced transactionally in `provider_config_repo::upsert` (activating one provider deactivates the rest). The frontend honours the explicit pick and never falls back to an arbitrary first row — see [`plan/versions/v1/CONNECTION_SELECT.md`](plan/versions/v1/CONNECTION_SELECT.md).

## Testing Guidelines

Frontend and shared package tests use Vitest and generally follow `*.test.ts` or colocated test naming. End-to-end coverage uses Playwright under `apps/desktop/e2e`. Rust unit tests are colocated with modules, with integration tests under `apps/desktop/src-tauri/tests`. Ollama integration tests are opt-in; set the required environment flags before running them. When changing schemas, prompts, provider payloads, or IPC contracts, add tests that cover malformed and model-specific edge cases.

## Commit & Pull Request Guidelines

Git history uses concise Conventional Commit style, such as `fix: ...`, `feat: ...`, `docs: ...`, or scoped variants like `fix(sandbox): ...`. PRs should include a short problem statement, the important implementation details, verification commands run, and screenshots or recordings for visible UI changes. Call out changes that affect model providers, sandbox behavior, persistence, or configuration.

`master` is squash-only and linear, gated by the `Protect master` ruleset. Six CI jobs must pass before the merge button unlocks — `conflict-marker-check`, `lint-and-test`, `frontend-checks`, `server-check`, `e2e-test`, `sandbox-runner-test`; `integration-test (ubuntu)` is advisory. Run `pnpm guard:pre-push` locally first. See [`docs/AGENT_WORKFLOW.md`](docs/AGENT_WORKFLOW.md) and [`BRANCH_PROTECTION.md`](BRANCH_PROTECTION.md).

## Security & Configuration Tips

Do not commit API keys, tokens, or user secrets. Keep local configuration in ignored environment files. The app is local-first and defaults to local providers where possible; document any new network access, sandbox permission, or secret-storage requirement in the PR.
