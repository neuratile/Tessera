# Tessera — Project Status & Context

> **Last updated**: 2026-06-05 | **Version**: v0.1.1+ (sandbox branch)
>
> A living document that gives anyone — new contributor, reviewer, or stakeholder —
> full context on what Tessera is, where it stands, and what's actively changing.

---

## What is Tessera?

**Tessera** is a **local-first AI testing IDE** that turns any codebase into a full
QA dossier without sending source to the cloud.

Open a folder → Tessera parses it with **Tree-sitter**, embeds chunks via **Ollama**,
indexes them in **SQLite** → click an artifact button → an LLM generates structured
QA artifacts (test plans, test cases, defect reports, bug reports) → output is
**Zod-validated** → you approve, reject, regenerate with feedback, or export.

**Three guarantees**: architecture-aware (RAG retrieves symbols across the whole
project) · static by default (analysis never executes your code) · structured
(every artifact is validated JSON).

### Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri 2.0 · Rust 1.81+ |
| Storage | SQLite 3 + `sqlite-vec` (embedded) |
| AST analysis | Tree-sitter (JS/TS/Python) |
| Frontend | React 19 + TypeScript + Vite + Tailwind v4 + shadcn/ui + Monaco |
| Monorepo | pnpm 10+ workspaces + Turborepo |
| LLM providers | Ollama (default), OpenAI, OpenRouter, Anthropic |
| Embeddings | Ollama `nomic-embed-text` (768-dim) |
| Test sandbox | Docker (opt-in) — vitest + istanbul |
| CI/CD | GitHub Actions (5-gate CI + cross-platform releases) |
| Observability | `tracing` (Rust) · Sentry (opt-in) |

---

## Codebase at a Glance

| Component | Files | Lines | Purpose |
|-----------|-------|-------|---------|
| Rust backend | 68 | ~15,700 | Commands → services → repositories → SQLite |
| React frontend | 69 | ~8,100 | Components, Zustand stores, typed IPC client |
| Shared (Zod) | 30+ | ~2,400 | Schemas + inferred TS types (FE/BE contract) |
| Tests | 14+ | ~2,000+ | Unit + integration + E2E + snapshot |
| Migrations | 4 | ~380 | Schema evolution (0001–0004) |
| CI/CD | 3 | ~350 | GitHub Actions workflows |

### Test Coverage

| Category | Count |
|----------|-------|
| Rust unit tests | 218+ |
| TypeScript unit tests | 43 |
| Zod schema/contract tests | 78 |
| Integration tests (live Ollama) | 2 |
| E2E tests (Playwright) | 2 |
| Snapshot tests (Insta) | 6 |
| **Total** | **349+** |

---

## Repository Layout

```
tessera/
├── apps/
│   └── desktop/                 Tauri desktop app
│       ├── src/                 React frontend
│       │   ├── components/      UI components (ai-panel, editor, file-explorer, layout, settings)
│       │   ├── stores/          Zustand stores (ai, auth, editor, sandbox, toast, ui, workspace)
│       │   ├── lib/ipc/         Typed IPC wrappers (16 modules, Zod-validated)
│       │   └── lib/             Utilities (command-bus, export, partial-json, etc.)
│       ├── src-tauri/           Rust backend
│       │   ├── src/commands/    Tauri IPC handlers (thin, validate, delegate)
│       │   ├── src/services/    Business logic (generation, sandbox, analysis, etc.)
│       │   ├── src/repositories/ Data access (parameterized SQL only)
│       │   ├── src/providers/   LLM (5 providers), embeddings, runners (Docker)
│       │   ├── src/prompts/     Versioned prompt templates
│       │   ├── src/auth/        JWT + Argon2 + AES-256-GCM
│       │   ├── src/db/          Schema init + migrations
│       │   ├── docker/          Dockerfile.runner-js (sandbox image)
│       │   └── docs/adr/        Architecture Decision Records
│       └── e2e/                 Playwright E2E tests
├── packages/
│   ├── shared/                  Zod schemas + inferred TS types (FE/BE contract)
│   ├── ui/                      shadcn/ui shared components
│   ├── eslint-config/           ESLint presets
│   └── tsconfig/                TypeScript presets
├── docs/                        Process & review documentation
├── plan/                        Planning documents (roadmap, feature plans)
├── rules/                       Engineering rulebook (rules.md)
├── tools/scripts/               Deploy, release, pre-push scripts
└── .github/workflows/           CI + release + auto-merge pipelines
```

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                    Tessera Desktop (Tauri)                     │
│                                                                │
│   React 19 + TS + Tailwind + shadcn/ui    ◀── Renderer        │
│            │  typed IPC (Zod-validated, kebab-case wire)       │
│            ▼                                                   │
│   Rust commands ─▶ services ─▶ repositories ─▶ SQLite + vec0  │
│            ├─▶ Tree-sitter (JS / TS / Python)                 │
│            ├─▶ Ollama embeddings (nomic-embed-text)            │
│            ├─▶ LLM provider trait (Ollama / OpenAI /          │
│            │                       OpenRouter / Anthropic)     │
│            └─▶ TestRunner trait (opt-in Docker sandbox, JS/TS)│
└────────────────────────────────────────────────────────────────┘
```

**Backend layering** (enforced by [`rules/rules.md`](../rules/rules.md) §4.2):

1. **Commands** — thin Tauri IPC handlers, validate inputs, delegate to services
2. **Services** — orchestrate RAG + prompts + validation, sole entry points
3. **Repositories** — only place that touches SQL, parameterized queries only
4. **Providers** — LLM/embedding/runner implementations behind traits

---

## What Has Shipped (v0.1.x)

### Core Features (v0.1.0)
- ✅ **5 QA artifact types**: Context, Test Plan, Test Cases, Defect Report, Bug Report
- ✅ **5 LLM providers**: Ollama Local (default), Ollama Cloud, OpenAI, OpenRouter, Anthropic
- ✅ **RAG pipeline**: Tree-sitter AST → semantic chunking → Ollama embeddings → SQLite vector search
- ✅ **Streaming generation** with partial-JSON preview and blinking caret
- ✅ **Prompt versioning** with JSON-Schema tool calls and insta snapshots
- ✅ **Artifact lifecycle**: generate → stream → approve/reject → regenerate with feedback → export to Markdown
- ✅ **Artifact versioning**: `parent_id` chain, dependency-free LCS line diff, version compare dropdown
- ✅ **First-run wizard**: hardware probe, Ollama connectivity test, model-pull check
- ✅ **AES-256-GCM** encrypted API key storage at rest
- ✅ **Command palette** with 13 commands and full keyboard nav
- ✅ **Cross-platform releases**: Windows, macOS, Linux via `tauri-action`
- ✅ **339+ tests** and 5-gate CI

### Sandbox Test Runner (shipped 2026-06-05)
- ✅ **Closed-loop test execution**: Generate test cases → execute in Docker → see pass/fail + line coverage
- ✅ **Hardened Docker sandbox**: `--network none`, `--cap-drop ALL`, non-root, read-only rootfs, resource caps
- ✅ **Opt-in, off by default**: backend rejects runs when flag is off (defence in depth)
- ✅ **Monaco coverage gutters**: green = covered, amber = uncovered
- ✅ **Run/Stop controls**: Cancel in-flight runs via `CancelToken`
- ✅ **Playwright E2E**: sandbox flow tested end-to-end

---

## What's Actively Changing

### In Progress: `feat/sandbox-test-runner` branch

The sandbox test runner has been built across 6 phases and is ready for merge to master.
All phases are complete:

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | ADR + Docker spike | ✅ Done |
| 1 | Contract schemas + migration | ✅ Done |
| 2 | Backend vertical slice | ✅ Done |
| 3 | Sandbox hardening (security gate) | ✅ Done |
| 4 | Coverage parse + storage | ✅ Done |
| 5 | Frontend (Run/Stop, results panel, gutters) | ✅ Done |
| 6 | Tests, docs, polish | ✅ Done |

**Files changed in this branch** (vs. master):
- **62 files** modified, **~2,341 insertions**, **~238 deletions**
- **~2,885 new lines of Rust** across 5 new backend files
- **~385 new lines of TypeScript** across 4 new frontend files
- **~1,123 new lines of documentation** across 4 docs/plan files

### In Progress: Bug Fixes

- `fix/artifact-output-harness` — active branch for artifact output improvements
- `fix/greptile-review-batch-1` — code review fixes

### Planned: JIRA Integration (Tessera Boards)

A comprehensive plan exists at [`plan/JIRA_INTEGRATION.md`](../plan/JIRA_INTEGRATION.md)
for adding Jira-like project management inside the IDE:
- New `apps/server` — Rust/Axum HTTP + WebSocket server
- PostgreSQL backend with real-time sync
- Teams → boards → columns → issues with drag-drop
- 5-phase implementation plan, not yet started

---

## Known Limitations

| Limitation | Impact | Status |
|---|---|---|
| **Brute-force vector search** | O(n) per query, caps at ~50K chunks | sqlite-vec deferred until 50K trigger (ADR-0002) |
| **Read-only editor** | Dirty tracking wired but no save action | Deferred (Phase 11) |
| **Ollama-only embeddings** | RAG quality limited without GPU | Cloud providers planned behind `EmbeddingProvider` trait |
| **Minimal E2E coverage** | 2 Playwright specs, no error-path tests | Expanding to 10–15 specs |
| **No export integrations** | Artifacts trapped in local SQLite | JIRA/Linear/GitHub connectors planned |
| **Vestigial auth** | Full JWT/Argon2 built, but single-user only | Scope decision pending (team flag vs strip) |
| **Single language sandbox** | Only JS/TS test runner | Python (`docker_py`) next, same `TestRunner` trait |

---

## Roadmap

See [`plan/ROADMAP.md`](../plan/ROADMAP.md) for the full roadmap.

### Next Up (priority order)

1. **Python test runner** — `docker_py.rs`, same trait, same tables, proves abstraction
2. **sqlite-vec ANN** — close the README/code gap on vector search
3. **Cloud embedding providers** — OpenAI / Voyage / Cohere behind existing trait
4. **Editor save** — Ctrl+S → `fs.writeTextFile`, clear dirty flag
5. **Test ↔ code traceability** — call graph from AST, stale-test detection
6. **Export connectors** — JIRA / Linear / GitHub Issues
7. **Multi-model consensus** — run same prompt across 2–3 providers, side-by-side compare

### Future Vision

- **Mutation testing** — mutate source, check if tests catch mutations, report mutation score
- **Diff-aware incremental generation** — watch git diffs, regenerate only affected test cases
- **Prompt Studio** — user-editable prompt templates with eval harness
- **Team collaboration** — workspace sync via CRDTs, optional cloud relay

---

## Quality Scorecard

| Dimension | Grade | Detail |
|-----------|:---:|--------|
| Architecture | A+ | Layered (commands → services → repos), trait-based providers |
| Type safety | A+ | Strict TypeScript, full Rust safety, Zod at every boundary |
| Error handling | A+ | Typed errors with stable codes, graceful degradation |
| Testing | A− | 349+ tests, live integration; thin E2E, no coverage reporting |
| Documentation | A | Excellent `rules.md` + ADRs; CHANGELOG + PROJECT_STATUS now added |
| Security | A+ | AES-GCM, Argon2, JWT, parameterized SQL, hardened sandbox |
| CI/CD | A+ | Multi-stage guards, cross-platform release, branch protection |
| Accessibility | B+ | 84+ aria attributes; no automated audit in CI |
| Code cleanliness | A+ | No TODO/FIXME in production, Clippy pedantic clean |

---

## Key Documents

| Document | Purpose |
|----------|---------|
| [README.md](../README.md) | Project overview, quick start, architecture |
| [CHANGELOG.md](../CHANGELOG.md) | Version history and detailed change log |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | How to contribute (short version) |
| [docs/AGENT_WORKFLOW.md](./AGENT_WORKFLOW.md) | Full change-management contract for humans + AI agents |
| [docs/FEATURE_REVIEW.md](./FEATURE_REVIEW.md) | Feature-by-feature scorecard and improvement priorities |
| [BRANCH_PROTECTION.md](../BRANCH_PROTECTION.md) | Admin branch protection setup |
| [CLAUDE.md](../CLAUDE.md) | AI coding agent guidance and project context |
| [rules/rules.md](../rules/rules.md) | Engineering rules (16 sections, 458 lines) |
| [plan/ROADMAP.md](../plan/ROADMAP.md) | Feature roadmap and known limitations |
| [plan/SANDBOX_TEST_RUNNER.md](../plan/SANDBOX_TEST_RUNNER.md) | Sandbox runner implementation plan (6 phases) |
| [plan/JIRA_INTEGRATION.md](../plan/JIRA_INTEGRATION.md) | Jira-like boards feature plan |

### Architecture Decision Records

| ADR | Decision |
|-----|----------|
| [ADR-0001](../apps/desktop/src-tauri/docs/adr/0001-blob-embeddings.md) | BLOB `f32` embeddings in SQLite |
| [ADR-0002](../apps/desktop/src-tauri/docs/adr/0002-vec0-migration-trigger.md) | vec0 migration trigger at 50K chunks |
| [ADR-0003](../apps/desktop/src-tauri/docs/adr/0003-llm-provider-trait.md) | LLM provider trait abstraction |
| [ADR-0004](../apps/desktop/src-tauri/docs/adr/0004-sandbox-test-runner.md) | Docker sandbox test runner |

---

## Active Branches

| Branch | Owner | Purpose | Status |
|--------|-------|---------|--------|
| `master` | — | Stable trunk | Green |
| `feat/sandbox-test-runner` | Rajveer | Sandbox test runner (Phases 1–6) | Ready for merge |
| `fix/artifact-output-harness` | Rajveer | Artifact output improvements | Active |
| `fix/greptile-review-batch-1` | — | Code review fixes | Active |
| `feat/gemini-provider` | — | Gemini LLM provider | In progress |

---

## Recent Contributors

| Contributor | Recent Work |
|-------------|-------------|
| **Rajveer Vadnal** (@Rajveerx11) | Sandbox runner (full stack), CI, docs, refactors |
| **Yuvraj Gandhmal** | Artifact generation fixes (#30, #32) |
| **ded-furby** | Pre-push Rust-optional fix (#27) |

---

## Getting Started

```bash
git clone https://github.com/Rajveerx11/Tessera.git tessera
cd tessera
corepack enable && corepack pnpm install
cp .env.example .env
pnpm bootstrap:ollama       # starts Ollama, pulls chat + embedding models
pnpm --filter @testing-ide/desktop run dev   # boots Vite + Tauri
```

For the full setup guide, see the [README](../README.md).
For contribution workflow, see [docs/AGENT_WORKFLOW.md](./AGENT_WORKFLOW.md).
