<div align="center">

<img src="apps/desktop/public/tessera-logo.png" alt="Tessera Logo" width="180" />

# Tessera

**Local-first AI testing IDE — turn any codebase into a full QA dossier without sending source to the cloud.**

[![CI](https://github.com/Rajveerx11/Tessera/actions/workflows/ci.yml/badge.svg)](https://github.com/Rajveerx11/Tessera/actions/workflows/ci.yml)
[![Release](https://github.com/Rajveerx11/Tessera/actions/workflows/release.yml/badge.svg)](https://github.com/Rajveerx11/Tessera/actions/workflows/release.yml)
[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24C8DB?logo=tauri)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-1.81+-CE422B?logo=rust)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react)](https://react.dev/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE.md)

</div>

> A **tessera** is one tile of a mosaic. Tessera the IDE assembles thousands of code chunks, AST nodes, and test cases into a single, reviewable picture of your software's quality.

---

## What it is

Tessera is a desktop IDE that runs **static-only** analysis on a codebase and uses an LLM to generate structured QA artifacts — test plans, test cases, defect reports, bug reports. Everything runs on your machine (local LLM, local SQLite, local AST parsing), so it works on closed-source, regulated, and offline codebases.

Open a folder → Tessera parses it with Tree-sitter, embeds chunks via Ollama, and indexes them in SQLite (`sqlite-vec`). Click an artifact button → the active LLM provider runs a versioned, JSON-Schema-constrained prompt over RAG-retrieved context. Output is validated against a Zod schema, then you approve, reject, regenerate-with-feedback, or export to Markdown. **Source never leaves the machine on the default Ollama provider.**

### Why it's different

| Tool | Generates code? | Generates QA docs? | Static analysis? | Closed-source? |
|------|:---:|:---:|:---:|:---:|
| Cursor / Copilot | Yes | No | Partial | Yes |
| Mabl / TestRigor | No | Limited | Runtime only | No |
| SonarQube | No | No | Rule-based | Yes |
| **Tessera** | **No (by design)** | **Yes** | **Tree-sitter + RAG** | **Yes (local LLM)** |

Three guarantees: **architecture-aware** (RAG retrieves symbols across the whole project, not just the open file) · **static-only** (code is never executed — safe for production / regulated repos) · **structured** (every artifact is validated JSON that exports cleanly to JIRA / Notion / GitHub Issues).

---

## Artifacts

| Type | Output |
|------|--------|
| **Context** | Architectural summary — the project memory for downstream artifacts |
| **Test Plan** | Scope, objectives, strategy, environments, risk matrix, entry/exit criteria |
| **Test Cases** | Steps, expected results, priority, traceability back to a source symbol |
| **Defect Report** | Static findings: severity, category, location, suggested fix, confidence |
| **Bug Report** | Potential runtime issues, formatted for ticket trackers |

Each artifact is versioned; regenerating with reviewer feedback bumps the version and links to its parent.

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                    Tessera Desktop (Tauri)                     │
│                                                                │
│   React 19 + TS + Tailwind + shadcn/ui    ◀── Renderer         │
│            │  typed IPC (Zod-validated, kebab-case wire)       │
│            ▼                                                   │
│   Rust commands ─▶ services ─▶ repositories ─▶ SQLite + vec0   │
│            ├─▶ Tree-sitter (JS / TS / Python)                  │
│            ├─▶ Ollama embeddings (nomic-embed-text)            │
│            └─▶ LLM provider trait (Ollama / OpenAI /           │
│                                    OpenRouter / Anthropic)     │
└────────────────────────────────────────────────────────────────┘
```

Layered backend (see [`rules/rules.md`](./rules/rules.md) §4.2): **commands** are thin Tauri IPC, **services** orchestrate RAG + prompts + validation, **repositories** are the only place that touches SQL, **providers** are LLM/embedding implementations behind a trait. API keys at rest are encrypted with AES-256-GCM derived from `JWT_SECRET`.

---

## Stack & providers

| Layer | Choice |
|-------|--------|
| Shell / backend | Tauri 2.0 · Rust 1.81+ (Tokio, sqlx, reqwest/rustls) |
| Storage | SQLite 3 + `sqlite-vec` (embedded, no daemon) |
| AST | `tree-sitter` — JS / TS / Python (more on the roadmap) |
| Frontend | React 19 + TypeScript + Vite + Tailwind v4 + shadcn/ui + Monaco |
| Observability | `tracing` logs · Sentry (opt-in, both sides) |

| LLM provider | Auth | Local | Notes |
|----------|------|:-----:|-------|
| **Ollama Local** | none | ✅ | Default — ships `qwen2.5-coder:7b` + `nomic-embed-text` |
| Ollama Cloud | API key | ❌ | Same wire format, hosted |
| OpenAI | API key | ❌ | Custom base URL (Azure / proxies) |
| OpenRouter | API key | ❌ | Gateway to many models |
| Anthropic | API key | ❌ | Claude family |

Embeddings are pluggable; the default `nomic-embed-text` (768-dim, Apache-2.0) ships with Ollama.

---

## Quick start

| Tool | Version | Notes |
|------|---------|-------|
| Rust | 1.81+ | [rustup.rs](https://rustup.rs/) + `clippy` + `rustfmt` |
| Node.js | 20+ | LTS |
| pnpm | 10+ | `corepack enable` |
| Ollama | latest | [ollama.com](https://ollama.com/) — local provider only |

```bash
git clone https://github.com/Rajveerx11/Tessera.git tessera
cd tessera
corepack enable && corepack pnpm install
cp .env.example .env
pnpm bootstrap:ollama                          # starts Ollama, pulls chat + embedding models
pnpm --filter @testing-ide/desktop run dev     # boots Vite + Tauri; the desktop window opens
```

- **macOS** — `xcode-select --install`.
- **Linux** — install Tauri's system deps: `libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev build-essential curl wget file`.
- **Optional shared stack** — `pnpm services:up` / `services:down` runs pgvector + Ollama via [`docker-compose.yml`](./docker-compose.yml).

---

## Configuration

The desktop app reads `apps/desktop/.env` (copy from [`apps/desktop/.env.example`](./apps/desktop/.env.example)); the root [`.env.example`](./.env.example) covers the optional Docker stack. Key variables:

- `OLLAMA_BASE_URL` — Ollama endpoint, default `http://localhost:11434`
- `JWT_SECRET` — required for auth paths; also derives the AES key for stored API keys
- `LOG_LEVEL` — `tracing` filter (`info`, `debug`, `tessera=trace`)
- `SENTRY_DSN` / `VITE_SENTRY_DSN` — error reporting (off when unset)

---

## Testing

```bash
pnpm test          # frontend Vitest + Rust unit tests
pnpm typecheck     # TypeScript across the monorepo
pnpm lint          # ESLint + clippy in CI

pnpm --filter @testing-ide/desktop run test:integration   # live Ollama suite
pnpm --filter @testing-ide/desktop run test:e2e           # Playwright desktop flow
```

Clippy runs clean under `-W clippy::pedantic`; release builds are green on Windows, macOS, and Linux via `tauri-action`.

---

## Repo layout

```
apps/desktop/        Tauri shell — React frontend (src/) + Rust backend (src-tauri/)
packages/
  shared/            Zod schemas + inferred TS types (the FE/BE contract)
  ui/                shadcn/ui-flavored shared components
  eslint-config/     base + React presets
  tsconfig/          base + desktop presets
rules/               engineering rulebook (rules.md)
docs/                workflow + process docs
tools/scripts/       deploy + release automation
.github/workflows/   CI + release pipelines
```

Architecture Decision Records live in [`apps/desktop/src-tauri/docs/adr/`](./apps/desktop/src-tauri/docs/adr/).

---

## Roadmap

**v0.1 (shipped)** — feature-complete: 5 artifact types, 5 LLM providers, RAG pipeline, streaming generation, first-run wizard, cross-platform signed releases.

**Next** — more AST languages (Go, Java, C#, Ruby, Rust) · `sqlite-vec` virtual-table search for projects > 50K chunks ([ADR-0002](./apps/desktop/src-tauri/docs/adr/0002-vec0-migration-trigger.md)) · cloud embedding providers · export to JIRA / Linear / GitHub Issues · team-mode collaboration.

Full roadmap + known limitations: [`plan/ROADMAP.md`](./plan/ROADMAP.md).

---

## Releases

Tag a commit to trigger the matrix build (Windows / macOS / Linux) via [`release.yml`](./.github/workflows/release.yml):

```bash
git tag v0.1.0 && git push origin v0.1.0
```

For a local bundle, run `bash tools/scripts/deploy.sh` (Git Bash on Windows).

---

## Contributing

Master is kept **green and linear** — PR-only, squash merge, branch protection. Husky hooks (conflict-marker + large-file guard on commit; typecheck + lint + test + clippy on push) auto-wire on `pnpm install`, and CI runs the same gauntlet.

```bash
git checkout -b feat/<short-slug>
# work, commit, then:
pnpm guard:pre-push        # optional — runs the full local gauntlet up front
git push -u origin HEAD
gh pr create --fill        # template + CODEOWNERS take it from here
```

Read before opening a PR:

- [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md) — the change-management contract (humans + AI agents)
- [`rules/rules.md`](./rules/rules.md) — engineering rules (layering, IPC, schema validation, security)
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) — quick-start pointer · [`BRANCH_PROTECTION.md`](./BRANCH_PROTECTION.md) — admin runbook

---

## License

[MIT](./LICENSE.md). Use it, fork it, ship it.

<div align="center">

Built locally. Runs locally. Reviews locally.<br/>
**Tessera** — the mosaic of your software's quality.

</div>
