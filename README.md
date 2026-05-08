<div align="center">

# Tessera

**Local-first AI testing IDE — turn any codebase into a full QA dossier without sending source to the cloud.**

[![CI](https://github.com/Rajveerx11/Tessera/actions/workflows/ci.yml/badge.svg)](https://github.com/Rajveerx11/Tessera/actions/workflows/ci.yml)
[![Release](https://github.com/Rajveerx11/Tessera/actions/workflows/release.yml/badge.svg)](https://github.com/Rajveerx11/Tessera/actions/workflows/release.yml)
[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24C8DB?logo=tauri)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-1.81+-CE422B?logo=rust)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react)](https://react.dev/)
[![Ollama](https://img.shields.io/badge/Ollama-local-000000?logo=ollama)](https://ollama.com/)
[![License: Pending](https://img.shields.io/badge/license-pending-lightgrey.svg)](#license)

</div>

> A **tessera** is one tile of a mosaic. Tessera the IDE assembles thousands of code chunks, AST nodes, and test cases into a single, reviewable picture of your software's quality.

---

## Table of contents

- [What it is](#what-it-is)
- [Why Tessera](#why-tessera)
- [Artifact types](#artifact-types)
- [Architecture](#architecture)
- [Tech stack](#tech-stack)
- [LLM providers](#llm-providers)
- [Quick start](#quick-start)
- [Environment](#environment)
- [Running the app](#running-the-app)
- [Testing](#testing)
- [Release builds](#release-builds)
- [Repo layout](#repo-layout)
- [Status & roadmap](#status--roadmap)
- [Workflow & guards](#workflow--guards)
- [Contributing](#contributing)
- [License](#license)

---

## What it is

Tessera is a desktop IDE that performs **static-only** analysis on a codebase and uses an LLM to generate structured QA artifacts: test plans, test cases, defect reports, and bug reports. It runs fully on your machine — local LLM, local SQLite, local AST parsing — so it works on closed-source code, regulated codebases, and offline networks.

- Open any folder via the native picker.
- Tessera walks the tree, parses source with Tree-sitter, embeds chunks with Ollama, and stores everything in an embedded SQLite database with a `sqlite-vec` index.
- Click an artifact button (Context, Test plan, Test cases, Defects, Bugs). The active LLM provider is invoked with a versioned prompt + JSON-Schema tool call.
- Every output is validated against a Zod schema in `packages/shared/` before it lands in the review queue.
- Approve, reject, regenerate-with-feedback, or export to Markdown.

Source code never leaves the machine when the local Ollama provider is selected.

---

## Why Tessera

| Tool | Generates code? | Generates QA docs? | Static analysis? | Works on closed-source? |
|------|----------------|--------------------|------------------|------------------------|
| Cursor / Copilot | Yes | No | Partial | Yes |
| Mabl / TestRigor | No | Limited | No (runtime UI only) | No |
| SonarQube | No | No | Yes (rule-based) | Yes |
| **Tessera** | **No (intentionally)** | **Yes — plans, cases, defects, bug reports** | **Yes (Tree-sitter + RAG)** | **Yes (local LLM)** |

Three guarantees:

1. **Architecture-aware.** Embeddings + RAG retrieve symbols across the project, not just the open file.
2. **Static-only.** Code is never executed. Safe for production / closed-source / regulated codebases.
3. **Structured outputs.** Every artifact is validated JSON. Renders to Markdown, exports cleanly to JIRA / Notion / GitHub Issues.

---

## Artifact types

| Type | Output |
|------|--------|
| **Context** | Architectural summary used as the project memory for downstream artifacts |
| **Test Plan** | Scope, objectives, strategy, environments, risk matrix, entry/exit criteria |
| **Test Cases** | Individual cases with steps, expected results, priority, and traceability back to a source symbol |
| **Defect Report** | Static-analysis findings: severity, category, location, suggested fix, confidence |
| **Bug Report** | Potential runtime issues, formatted for ticket trackers |

Each artifact is versioned. Regenerating with reviewer feedback bumps the version and links back to the parent.

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                    Tessera Desktop (Tauri)                     │
│                                                                │
│   React 19 + TS + Tailwind + shadcn/ui    ◀── Renderer         │
│            │                                                   │
│            │  typed IPC (Zod-validated, kebab-case wire)       │
│            ▼                                                   │
│   Rust commands ─▶ services ─▶ repositories ─▶ SQLite + vec0   │
│            │                                                   │
│            ├─▶ Tree-sitter (JS / TS / Python)                  │
│            ├─▶ Ollama embeddings (nomic-embed-text)            │
│            └─▶ LLM provider trait (Ollama / OpenAI /           │
│                                    OpenRouter / Anthropic)     │
└────────────────────────────────────────────────────────────────┘
```

Layered backend per [`rules/rules.md`](./rules/rules.md) §4.2:

- **Commands** — thin Tauri IPC, no business logic
- **Services** — orchestration, RAG, prompt building, schema validation
- **Repositories** — only place that touches SQL + BLOBs
- **Providers** — LLM + embedding implementations behind a trait

API keys at rest are encrypted with AES-256-GCM, derived from `JWT_SECRET`.

---

## Tech stack

| Layer | Choice |
|-------|--------|
| Shell | Tauri 2.0 (~3 MB native binary, real filesystem access) |
| Backend | Rust 1.81+ (Tokio, sqlx, reqwest with rustls TLS) |
| Storage | SQLite 3 + `sqlite-vec` (embedded, no daemon) |
| AST | `tree-sitter` (JS / TS / Python; more languages on the roadmap) |
| Frontend | React 19 + TypeScript + Vite + TailwindCSS v4 + shadcn/ui |
| Editor | Monaco (offline-bundled, VS Code parity) |
| Streaming | Tauri events on `generation://event` for live tool-call previews |
| Logging | `tracing` (pretty in dev, JSON in release) |
| Telemetry | Sentry (opt-in on both Rust and React) |

---

## LLM providers

| Provider | Auth | Local | Notes |
|----------|------|:-----:|-------|
| **Ollama Local** | none | ✅ | Default. Ships with `qwen2.5-coder:7b` chat + `nomic-embed-text` embeddings |
| Ollama Cloud | API key | ❌ | Same wire format, hosted |
| OpenAI | API key | ❌ | Custom base URL supported (Azure / proxies) |
| OpenRouter | API key | ❌ | Gateway to many open + proprietary models |
| Anthropic | API key | ❌ | Claude family |

Embedding model is pluggable. The default `nomic-embed-text` (768-dim, Apache-2.0) ships with Ollama and runs anywhere.

---

## Quick start

### Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Rust | 1.81+ | Install via [rustup.rs](https://rustup.rs/) and add `clippy` + `rustfmt` |
| Node.js | 20+ | LTS recommended |
| pnpm | 10+ | `corepack enable` |
| Ollama | latest | [ollama.com](https://ollama.com/) — only needed for the local provider |
| Docker | optional | Required only for the bundled `postgres + ollama` compose stack |

**Platform notes**

- **Windows** — supported out of the box.
- **macOS** — install Xcode Command Line Tools (`xcode-select --install`).
- **Ubuntu / Debian** — install Tauri system packages:

  ```bash
  sudo apt-get update
  sudo apt-get install -y \
    libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
    libayatana-appindicator3-dev librsvg2-dev \
    build-essential curl wget file
  ```

### Clone + run

```bash
git clone https://github.com/Rajveerx11/Tessera.git tessera
cd tessera
corepack enable
corepack pnpm install
cp .env.example .env
pnpm bootstrap:ollama          # starts Ollama, pulls chat + embedding models
pnpm --filter @testing-ide/desktop run dev
```

What happens:

- `corepack pnpm install` installs the monorepo (workspace + Turbo)
- `pnpm bootstrap:ollama` checks for Ollama, starts it if needed, and pulls `qwen2.5-coder:7b` + `nomic-embed-text`
- `pnpm --filter @testing-ide/desktop run dev` boots Vite and the Tauri shell — the desktop window opens automatically

Cold start to the first generated artifact is well under 10 minutes on a clean machine with Rust, Node, and Ollama already installed.

### Optional shared services

```bash
pnpm services:up      # starts pgvector + ollama via docker-compose
pnpm services:down
```

See [`docker-compose.yml`](./docker-compose.yml) for the full stack.

---

## Environment

The desktop app reads `apps/desktop/.env`. Create one from the example:

```bash
cp apps/desktop/.env.example apps/desktop/.env
```

A separate root [`.env.example`](./.env.example) covers the optional Docker Compose stack.

Useful variables:

| Variable | Purpose |
|----------|---------|
| `OLLAMA_BASE_URL` | Ollama endpoint, default `http://localhost:11434` |
| `LOG_LEVEL` | `tracing` filter, e.g. `info`, `debug`, `tessera=trace` |
| `JWT_SECRET` | Required for any auth-touching path; also drives the AES key for stored API keys |
| `SENTRY_DSN` | Native Rust / Tauri error reporting (server-side only, not bundled) |
| `VITE_SENTRY_DSN` | React / browser error reporting (public by design) |

If either Sentry DSN is unset, that side of the app stays offline and reports nothing.

---

## Running the app

Desktop development:

```bash
pnpm --filter @testing-ide/desktop run dev
```

Frontend-only build (sanity-check without spawning Tauri):

```bash
pnpm --filter @testing-ide/desktop run build
```

---

## Testing

The whole monorepo:

```bash
pnpm test
```

Targeted commands:

```bash
pnpm lint                                                         # ESLint + clippy in CI
pnpm typecheck                                                    # TypeScript across the monorepo
pnpm --filter @testing-ide/desktop run test                       # frontend Vitest + Rust unit tests
pnpm --filter @testing-ide/desktop run test:integration           # live Ollama integration suite
pnpm --filter @testing-ide/desktop run e2e:install                # Playwright browsers
pnpm --filter @testing-ide/desktop run test:e2e                   # full desktop flow under the test harness
```

Coverage at a glance:

- **Rust unit + Zod contract**: 230+ tests in `apps/desktop/src-tauri` and `packages/shared`
- **Clippy**: clean under `-W clippy::pedantic`
- **Audit**: 21 advisories triaged in [`audit.toml`](./audit.toml)
- **Release build**: green on Windows, macOS, Linux via `tauri-action`

---

## Release builds

Local desktop release bundle:

```bash
bash tools/scripts/deploy.sh
```

(Run from Git Bash on Windows.) The script:

- verifies required tooling
- installs dependencies if `node_modules/` is missing
- runs the Tauri production build
- preserves Tauri signing when signing env vars are set, and falls back to unsigned bundles otherwise
- copies release artifacts into `dist/desktop/`

### GitHub Releases

Tag pushes trigger [`.github/workflows/release.yml`](./.github/workflows/release.yml), which uses `tauri-apps/tauri-action` to build matrix bundles for Windows, macOS, and Linux and attaches them to a draft GitHub Release.

```bash
git tag v0.1.0
git push origin v0.1.0
```

Maintainer reviews the draft release notes and publishes.

---

## Repo layout

```
.
├── apps/
│   └── desktop/              # Tauri shell (Rust + React)
│       ├── src/              # React 19 + TS + Tailwind
│       └── src-tauri/        # Rust backend (commands → services → repositories)
├── packages/
│   ├── shared/               # Zod schemas + inferred TS types (FE/BE contract)
│   ├── ui/                   # shadcn/ui-flavored shared components
│   ├── eslint-config/        # base + React presets
│   └── tsconfig/             # base + desktop presets
├── tools/scripts/            # deploy + release automation
├── plan/                     # phase plans, ADRs, design docs
├── rules/                    # engineering rulebook
└── .github/workflows/        # CI + release pipelines
```

---

## Status & roadmap

Phases 1–13 are shipped. Tessera is feature-complete for the v0.1 milestone.

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | Tauri scaffold, layered structure, typed config + errors, SQLite + migrations | Shipped ([PR #2](https://github.com/Rajveerx11/Tessera/pull/2)) |
| 2 | LLM provider abstraction (Ollama / OpenAI / OpenRouter / Anthropic) + embeddings + factory | Shipped ([PR #3](https://github.com/Rajveerx11/Tessera/pull/3)) |
| 3 | AST pipeline: file discovery, Tree-sitter parsing, semantic chunking, chunk repository | Shipped ([PR #6](https://github.com/Rajveerx11/Tessera/pull/6)) |
| 4 | Versioned prompt templates with JSON-Schema function calling | Shipped ([PR #9](https://github.com/Rajveerx11/Tessera/pull/9)) |
| 5 | Generation service (RAG + prompts + LLM) | Shipped ([PR #10](https://github.com/Rajveerx11/Tessera/pull/10)) |
| 6 | Tauri IPC commands + AES-GCM API-key encryption | Shipped |
| 7 | Live Ollama integration tests, prompt snapshots, CI workflow | Shipped |
| 8 | Frontend IPC client + first-run wizard | Shipped |
| 9 | Workspace shell — three-panel layout, native folder dialog, file explorer | Shipped |
| 10 | Monaco editor + tab system + file content reads | Shipped |
| 11 | AI panel + Settings sheet + 4-step wizard + artifact lifecycle IPC | Shipped |
| 12 | Markdown preview drawer + regenerate-with-feedback | Shipped |
| 13 | Auto-analyze on open + Ollama model check + streaming events | Shipped |

**On the roadmap**

- More AST languages (Go, Java, C#, Ruby, Rust)
- `sqlite-vec` virtual-table search for projects > 50K chunks (ADR-0002)
- Cloud embedding providers (OpenAI, Voyage AI) behind the same trait
- Export to JIRA / Linear / GitHub Issues
- Team-mode collaboration (out-of-scope for v0.1)

---

## Workflow & guards

Tessera is built by a small team. Master is kept green and linear by a
three-layer defence so a broken push from one contributor never costs
the rest of the team a 1–2 hour cleanup session.

```
   commit ──► .husky/pre-commit                    (instant, on dev machine)
                ├─ conflict-marker scan
                └─ 5 MB large-file guard

   push ────► .husky/pre-push  →  pre-push.sh      (~30s–2min, on dev machine)
                ├─ conflict-marker scan
                ├─ pnpm typecheck
                ├─ pnpm lint
                ├─ pnpm test
                └─ cargo clippy + cargo test --lib  (if cargo installed)

   open PR ─► GitHub Actions                       (on the runner)
                ├─ conflict-marker-check
                ├─ lint
                ├─ typecheck
                ├─ unit-test
                ├─ integration-test (live Ollama)
                └─ release-build (Tauri matrix)

   merge ───► branch protection on `master`        (hard server-side gate)
                ├─ PR required, approval required (CODEOWNERS-routed)
                ├─ all required checks green
                ├─ branch up to date with master
                ├─ linear history (squash merge only)
                └─ no force-push, no admin bypass
```

The hooks wire up automatically the first time a contributor runs
`pnpm install` (Husky's `prepare` script). No manual hook install
needed. CI runs the same gauntlet on every PR, so what passes locally
passes in CI.

**Opt-in auto-merge.** Add the `auto-merge` label to a PR and
`.github/workflows/auto-merge.yml` flips GitHub-native auto-merge.
Once reviews + checks are green, the PR squash-merges with no manual
click. The label does not bypass any required gate.

Detailed references:

- [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md) — canonical
  workflow contract for AI agents and humans (hard rules, common
  failure modes, where-to-look table)
- [`BRANCH_PROTECTION.md`](./BRANCH_PROTECTION.md) — admin runbook for
  the GitHub branch-protection settings (apply once)
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) — short pointer for human
  contributors

---

## Contributing

Before changing code, read:

- [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md) — workflow contract
- [`rules/rules.md`](./rules/rules.md) — engineering rules (layering, IPC, schema validation, security)
- [`plan/initial-plan.md`](./plan/initial-plan.md) — phase plans + intent

The repo enforces:

- strict TypeScript across the monorepo
- Rust `clippy::pedantic` as a CI gate
- Zod validation at every trust boundary (IPC + form input)
- local-first AI workflows — no cloud dependency for the default path
- live Ollama integration coverage in CI

To get going:

```bash
git clone https://github.com/Rajveerx11/Tessera.git tessera
cd tessera
corepack enable
corepack pnpm install      # also wires the Husky pre-commit + pre-push hooks
git checkout -b feat/<short-slug>
# work, commit, then:
pnpm guard:pre-push         # optional — runs the full local gauntlet up front
git push -u origin HEAD
gh pr create --fill         # template + CODEOWNERS take it from here
```

---

## License

License is still pending. Until then, treat the repository as **all-rights-reserved**. A permissive license will be confirmed before the first tagged public release.

---

<div align="center">

Built locally. Runs locally. Reviews locally.<br/>
**Tessera** — the mosaic of your software's quality.

</div>
