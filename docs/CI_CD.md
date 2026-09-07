# Testing and CI/CD

## Local checks

Install with `pnpm install --frozen-lockfile`, then run `pnpm guard:pre-push`
before pushing. The pre-push hook runs it again.

The guard checks conflict markers, tooling tests, review fixtures, TypeScript,
ESLint, shared/frontend unit tests, then desktop Rust Clippy and unit tests
when Cargo is available.

This is not full CI parity. The guard omits server tests, the production
renderer build, Playwright, Docker execution, coverage, and live Ollama.
Missing Cargo skips local Rust checks; CI still requires them.

| Command | Coverage |
|---|---|
| `pnpm test:tooling` | Release-ref/version validator regression tests |
| `pnpm test:eval-fixtures` | Checkout baseline, seeded regression, clean control |
| `pnpm typecheck`, `pnpm lint` | Workspace TypeScript and ESLint |
| `pnpm test` | Tooling, fixtures, workspace tests including desktop Rust |
| `pnpm --filter @testing-ide/desktop test:frontend` | Renderer unit tests |
| `pnpm --filter @testing-ide/desktop test:rust` | Desktop Rust library tests |
| `pnpm --filter @testing-ide/desktop vite:build` | Production renderer build |
| `pnpm --filter @testing-ide/desktop e2e:install` | Install Playwright browser |
| `pnpm --filter @testing-ide/desktop test:e2e` | Renderer E2E with mocked Tauri IPC |
| `cargo test --manifest-path apps/server/Cargo.toml --locked` | API server tests |

See the [workflow](../.github/workflows/ci.yml) and colocated tests for Docker
and live-provider flags. Fixture tests require neither Docker nor a model and
do not execute a Tessera review. On Windows, use the
[shell troubleshooting guide](./GETTING_STARTED.md#windows-shell-troubleshooting).
A shell banner is not proof that TypeScript or ESLint executed.

## Required jobs

CI runs on PRs, pushes to master/main, manual dispatch, and reusable calls from
release. There are no docs-only skips; all six required names are preserved.

| Job | Checks |
|---|---|
| `conflict-marker-check` | Marker scan, tooling tests, checkout fixtures |
| `lint-and-test` | ESLint, desktop Clippy, workspace unit tests |
| `frontend-checks` | TypeScript and production Vite build |
| `server-check` | Server Clippy and tests |
| `e2e-test` | Playwright renderer flows |
| `sandbox-runner-test` | Docker image and gated JS/TS + Python runner tests |

Coverage and `integration-test (ubuntu)` remain advisory. Investigate failures:
infrastructure outages and real regressions can both cause them.

[Branch protection](../BRANCH_PROTECTION.md) requires six successful jobs and
squash merging. Auto-merge remains opt-in and never bypasses repository rules.

## Release flow

Preflight accepts only stable tags matching the desktop package, Rust crate,
and Tauri config, and checks that the commit belongs to master. Release reuses
the full CI workflow before any installer job. Only bundle jobs receive write
permission to create/update draft releases.

Linux, macOS universal, and Windows installers attach to the draft for manual
review. Signing/notarization are not configured. See [Releasing](./RELEASING.md).
