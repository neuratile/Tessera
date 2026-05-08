# Testing IDE

Desktop-first, local-first AI testing workspace built with Tauri, React, Rust, SQLite, and Ollama.

The happy path for a new developer is:

1. Clone the repo
2. Install workspace dependencies
3. Bootstrap Ollama models
4. Run the Tauri desktop app

If your machine already has Rust, Node, and Ollama installed, you can get to a working app in well under 10 minutes.

## Prerequisites

Install these before you start:

- `Rust 1.81+` via [rustup](https://rustup.rs/)
- `Node.js 20+`
- `pnpm 10+` via `corepack enable`
- `Ollama` via [ollama.com](https://ollama.com/)
- `Docker` if you want the optional shared `postgres + ollama` services

Platform notes:

- Windows: supported out of the box
- macOS: install Xcode Command Line Tools
- Ubuntu/Debian: install Tauri system packages:

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  build-essential \
  curl \
  wget \
  file
```

<<<<<<< HEAD
## Quick Start
=======
| Type | Output |
|------|--------|
| Test Plan | Scope, objectives, strategy, environments, risk matrix, entry/exit criteria |
| Test Cases | Individual cases with steps, expected results, priority, traceability to source |
| Defect Report | Static-analysis findings: severity, category, location, suggested fix, confidence |
| Bug Report | Potential runtime issues formatted for tracking |
| Test Summary | Executive coverage assessment with risk areas + recommendations |

---

## Why this exists

No existing tool owns the "static code analysis → full test strategy" space:

- **Cursor / Copilot** generate test code snippets but treat testing as an add-on, not a primary product.
- **Mabl / TestRigor** automate end-to-end UI testing but require a running application — they cannot reason about closed-source code or generate test plans before the system ships.
- **SonarQube** detects code-quality issues but does not produce test plans, test cases, or structured QA documents.

This IDE bridges the gap with three guarantees:

1. **Architecture-aware** — analyzes data flows + dependency graphs, not isolated files.
2. **Static analysis only** — works on any codebase, including production / closed-source. Code never executed.
3. **Structured outputs** — Markdown / JSON / JIRA-compatible artifacts, not just snippets.

---

## Status

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Foundation: Tauri scaffold, layered structure, typed config + errors, SQLite + migrations | **Shipped** ([PR #2](https://github.com/Rajveerx11/Testing-IDE/pull/2)) |
| 2 | LLM provider abstraction: Ollama / OpenAI / OpenRouter / Anthropic + Ollama embeddings + factory | **Shipped** ([PR #3](https://github.com/Rajveerx11/Testing-IDE/pull/3)) |
| 3 | AST pipeline: file discovery, Tree-sitter parsing, semantic chunking, chunk repository | **Shipped** ([PR #6](https://github.com/Rajveerx11/Testing-IDE/pull/6)) |
| 4 | Versioned prompt templates with JSON-Schema function calling | **Shipped** ([PR #9](https://github.com/Rajveerx11/Testing-IDE/pull/9)) |
| 5 | Generation service tying RAG + prompts + LLM | **Shipped** ([PR #10](https://github.com/Rajveerx11/Testing-IDE/pull/10)) |
| 6 | Tauri IPC commands + AES-GCM API-key encryption | **Shipped** (merged direct to `master` — commit `dc4d7d4`) |
| 7 | Integration tests against Ollama, snapshot tests for prompts, CI workflow | **Shipped** (merged direct to `master`) |
| 8 | Frontend IPC client + first-run wizard | **Shipped** (merged direct to `master`) |
| 9 | Workspace shell — three-panel layout, native folder dialog, file explorer | **Shipped** (merged direct to `master`) |
| 10 | Monaco editor + tab system + file content reads | **Shipped** (merged direct to `master`) |
| 11 | AI panel + Settings sheet + 4-step wizard + artifact lifecycle IPC | **Shipped** (merged direct to `master`) |
| 12 | Markdown preview drawer + regenerate-with-feedback | **Shipped** (merged direct to `master`) |
| 13 | Auto-analyze on open + Ollama model check + streaming events | **Shipped** (merged direct to `master`) |

**Parallel streams shipped:**
- **Monorepo** — pnpm workspaces + Turborepo at root. `packages/shared/` (Zod schemas + TS types for FE/BE contracts), `packages/eslint-config/`, `packages/tsconfig/`, `packages/ui/`. Single source of truth for types is the Rust serde-derived data layer; Zod schemas mirror per `rules.md` §12.3.1.
- **Frontend skeleton** — `apps/desktop/src/` Vite + React 19 + TailwindCSS v4 + shadcn/ui scaffold (App.tsx, main.tsx, button.tsx). Wired to Tauri's `init_db` and `greet` commands.
- **Tauri build pipeline** — `tauri.conf.json` carries `beforeDevCommand` + `beforeBuildCommand` hooks; CSP allows the Vite dev server at `localhost:5173`.

**Tests**: 231 Rust unit + Zod contract tests in `packages/shared/`. **Clippy**: clean (`pedantic` enforced). **Audit**: 21 advisories triaged in `audit.toml`. **Release build**: green.

---

## Tech stack

| Layer | Choice |
|-------|--------|
| Shell | Tauri 2.0 (~3 MB binary, native filesystem access) |
| Backend | Rust 1.77+ (Tokio async runtime, sqlx, reqwest with rustls TLS) |
| Storage | SQLite 3 + `sqlite-vec` (embedded, no daemon, no setup) |
| AST | `tree-sitter` (JS / TS / Python grammars wired via `services/ast_service.rs`; more languages in Phase 3.5+) |
| Frontend | React 19 + TypeScript + Vite + TailwindCSS v4 + shadcn/ui |
| Editor | Monaco (VS Code parity for code viewing) |
| Logging | `tracing` (pretty in dev, JSON in release) |

### LLM providers (all optional, user-configurable)

| Provider | Auth | Local? | Default? |
|----------|------|--------|----------|
| Ollama Local | none | yes | yes — runs `qwen2.5-coder:7b` out of the box |
| Ollama Cloud | API key | no | no |
| OpenAI | API key | no | no — supports custom base URLs (Azure / proxies) |
| OpenRouter | API key | no | no — gateway to many open + proprietary models |
| Anthropic | API key | no | no — Claude family |

**Embeddings**: `nomic-embed-text` (768 dim) via Ollama by default. Cloud embedding providers (OpenAI, Voyage AI) follow at the same trait shape.

---

## Quick start

### Prerequisites

- **Rust 1.77+** — install from [rustup.rs](https://rustup.rs/), then `rustup component add clippy rustfmt`.
- **Node.js 20+** + **pnpm** — for the frontend (Phase 1 ships backend only; frontend lands later).
- **Ollama** — install from [ollama.com](https://ollama.com/) for local LLM inference. Optional if you only use cloud providers.
- **Platform deps**: Windows is fully supported. Linux requires `webkit2gtk-4.1`. macOS requires Xcode CLI tools.

### Clone + build
>>>>>>> 4c47d2aa1ccf6ef1885b16104e3665fca6828162

```bash
git clone https://github.com/Rajveerx11/Testing-IDE.git
cd Testing-IDE
corepack enable
corepack pnpm install
cp .env.example .env
pnpm bootstrap:ollama
pnpm --filter @testing-ide/desktop run dev
```

What that does:

- `corepack pnpm install` installs the monorepo
- `pnpm bootstrap:ollama` checks for `ollama`, starts it if needed, and pulls:
  - `qwen2.5-coder:7b`
  - `nomic-embed-text`
- `pnpm --filter @testing-ide/desktop run dev` starts the Vite frontend and Tauri desktop shell

Optional shared services:

```bash
pnpm services:up
```

That starts the root-level Docker Compose stack in [docker-compose.yml](./docker-compose.yml):

- `postgres` using `pgvector/pgvector:pg16`
- `ollama` using `ollama/ollama:latest`

Stop it with:

```bash
pnpm services:down
```

## Environment Setup

The desktop app reads environment variables from [`apps/desktop/.env.example`](./apps/desktop/.env.example).

Create your local desktop env file:

```bash
cp apps/desktop/.env.example apps/desktop/.env
```

There is also a root [.env.example](./.env.example) for Docker Compose and shared local service values.

Useful variables:

- `OLLAMA_BASE_URL=http://localhost:11434`
- `LOG_LEVEL=info`
- `JWT_SECRET=...` for anything beyond local-only dev
- `SENTRY_DSN=...` enables native Rust/Tauri error reporting
- `VITE_SENTRY_DSN=...` enables React/browser-side error reporting

Notes:

- `SENTRY_DSN` stays on the Rust side and is not bundled into the frontend
- `VITE_SENTRY_DSN` is public by design and safe to expose to the client bundle
- If either Sentry DSN is omitted, that side of the app stays offline and does not report events

## Running the App

Desktop development:

```bash
pnpm --filter @testing-ide/desktop run dev
```

Frontend-only build:

```bash
pnpm --filter @testing-ide/desktop run build
```

## Test Commands

Run the whole monorepo test pipeline:

```bash
pnpm test
```

Common day-to-day commands:

```bash
pnpm lint
pnpm typecheck
pnpm --filter @testing-ide/desktop run test
pnpm --filter @testing-ide/desktop run test:integration
pnpm --filter @testing-ide/desktop run e2e:install
pnpm --filter @testing-ide/desktop run test:e2e
```

What they cover:

- `pnpm lint`: workspace ESLint plus Rust clippy in CI
- `pnpm typecheck`: TypeScript checks across the monorepo
- `pnpm --filter @testing-ide/desktop run test`: frontend Vitest + Rust unit tests
- `pnpm --filter @testing-ide/desktop run test:integration`: live Ollama integration tests
- `pnpm --filter @testing-ide/desktop run test:e2e`: Playwright desktop flow using the test harness

## Release Build

To build a local desktop release bundle:

```bash
bash tools/scripts/deploy.sh
```

On Windows, run that from Git Bash.

The deploy script:

- verifies required tooling
- installs dependencies if `node_modules/` is missing
- runs the Tauri production build
- lets Tauri sign artifacts when signing credentials are present
- copies release bundles into `dist/desktop/`

Signing behavior:

- if signing-related env vars are present, the script leaves signing enabled for Tauri
- if not, it still builds unsigned bundles so local release testing is not blocked

The script lives at [`tools/scripts/deploy.sh`](./tools/scripts/deploy.sh).

## GitHub Releases

Tag pushes trigger the Tauri release workflow in [`.github/workflows/release.yml`](./.github/workflows/release.yml).

That workflow uses `tauri-apps/tauri-action` and publishes a draft GitHub Release with platform bundles attached.

Typical release flow:

```bash
git tag v0.1.0
git push origin v0.1.0
```

After the tag push:

1. GitHub Actions runs the release workflow
2. Tauri builds bundles for Windows, macOS, and Linux
3. A draft GitHub Release is created with the artifacts attached
4. Maintainer reviews the draft notes and publishes it

## Repo Layout

Main directories:

- [`apps/desktop`](./apps/desktop): Tauri desktop app
- [`apps/desktop/src-tauri`](./apps/desktop/src-tauri): Rust backend
- [`packages/shared`](./packages/shared): shared Zod schemas and types
- [`packages/ui`](./packages/ui): shared UI package
- [`tools/scripts`](./tools/scripts): repo automation scripts
- [`plan`](./plan): planning docs
- [`rules`](./rules): engineering rules

## Sentry

Sentry is now initialized in both runtimes:

- React entrypoint: [`apps/desktop/src/lib/sentry.ts`](./apps/desktop/src/lib/sentry.ts)
- Rust/Tauri startup: [`apps/desktop/src-tauri/src/utils/telemetry.rs`](./apps/desktop/src-tauri/src/utils/telemetry.rs)

Both are opt-in and remain disabled until you set the matching DSN.

## Development Rules

Before changing code, read:

- [`plan/initial-plan.md`](./plan/initial-plan.md)
- [`rules/rules.md`](./rules/rules.md)

The repo follows:

- strict TypeScript
- Rust `clippy` as a gate
- Zod validation at trust boundaries
- local-first AI workflows
- Ollama-backed integration coverage

## License

License is still pending. Until then, treat the repository as all-rights-reserved.
