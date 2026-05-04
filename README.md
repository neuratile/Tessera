# Testing IDE

> AI-powered desktop IDE focused exclusively on software-test artifact generation.
>
> Upload a project folder. The IDE analyzes its structure, data flows, and dependencies, then generates structured test artifacts — test plans, test cases, defect reports, bug reports, test summary reports — for human review and approval.

[![Master CI](https://img.shields.io/badge/master-passing-brightgreen)](https://github.com/Rajveerx11/Testing-IDE)
[![Rust 1.77+](https://img.shields.io/badge/rust-1.77%2B-orange)](https://rustup.rs/)
[![License](https://img.shields.io/badge/license-TBD-lightgrey)](#license)

---

## Table of Contents

- [What it does](#what-it-does)
- [Why this exists](#why-this-exists)
- [Status](#status)
- [Tech stack](#tech-stack)
- [Quick start](#quick-start)
- [LLM provider configuration](#llm-provider-configuration)
- [RAG pipeline](#rag-pipeline)
- [Project structure](#project-structure)
- [Engineering rules](#engineering-rules)
- [Architecture decisions](#architecture-decisions)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)

---

## What it does

The IDE replaces hours of manual QA artifact authoring with an AI-driven static-analysis pipeline:

1. **Upload a project folder** — JavaScript / TypeScript / Python source today, more languages later.
2. **Static analysis** — Tree-sitter AST parsing extracts functions, classes, imports, and exports. No execution — uploaded code is never run.
3. **Semantic chunking + RAG** — code split at function / class boundaries, embedded with `nomic-embed-text` (or any configured provider), stored in SQLite + sqlite-vec.
4. **Hierarchical context generation** — bottom-up summarization produces a `context.md` describing the project's architecture and data flows.
5. **Artifact generation** — choose an artifact type and scope; the IDE assembles relevant chunks, calls your selected LLM, streams structured output back.
6. **Human-in-the-loop review** — approve, reject with feedback, or regenerate. Export as Markdown (PDF / JIRA / JSON in V1).

### Artifact types

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
| 4 | Versioned prompt templates with JSON-Schema function calling | Pending |
| 5 | Generation service tying RAG + prompts + LLM | Pending |
| 6 | Tauri IPC commands + AES-GCM API-key encryption | In progress — `init_db` command + `setup` lifecycle hook landed |
| 7 | Integration tests against Ollama, snapshot tests for prompts, CI workflow | Pending |

**Parallel streams shipped:**
- **Monorepo** — pnpm workspaces + Turborepo at root. `packages/shared/` (Zod schemas + TS types for FE/BE contracts), `packages/eslint-config/`, `packages/tsconfig/`, `packages/ui/`. Single source of truth for types is the Rust serde-derived data layer; Zod schemas mirror per `rules.md` §12.3.1.
- **Frontend skeleton** — `apps/desktop/src/` Vite + React 19 + TailwindCSS v4 + shadcn/ui scaffold (App.tsx, main.tsx, button.tsx). Wired to Tauri's `init_db` and `greet` commands.
- **Tauri build pipeline** — `tauri.conf.json` carries `beforeDevCommand` + `beforeBuildCommand` hooks; CSP allows the Vite dev server at `localhost:5173`.

**Tests**: 138 Rust unit + Zod contract tests in `packages/shared/`. **Clippy**: clean (`pedantic` enforced). **Audit**: 21 advisories triaged in `audit.toml`. **Release build**: green.

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

```bash
git clone https://github.com/Rajveerx11/Testing-IDE.git
cd Testing-IDE/apps/desktop/src-tauri

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo build --release --lib
```

All four commands must exit 0. CI enforces this on every push.

### Pull local LLM models (optional but recommended)

```bash
ollama pull qwen2.5-coder:7b      # 4.7 GB chat model — runs on 8 GB VRAM or 16 GB RAM CPU-only
ollama pull nomic-embed-text       # 274 MB embeddings — runs anywhere
```

### Configuration

Copy the env template and adjust as needed:

```bash
cd apps/desktop
cp .env.example .env
```

Defaults are sane for local development:

```
OLLAMA_BASE_URL=http://localhost:11434
DB_PATH=./testing-ide.db
LOG_LEVEL=info
```

### Run security audit

```bash
cd apps/desktop/src-tauri
cargo install cargo-audit --locked          # one-time
cargo audit                                  # see audit.toml for triaged advisories
```

---

## LLM provider configuration

Providers are selected at runtime via the factory in `src/providers/factory.rs`. The same trait powers every implementation, so service code never branches on provider identity.

```rust
use testing_ide_lib::providers::factory::{
    build_llm_provider, ProviderConfig, ProviderKind,
};

let cfg = ProviderConfig {
    kind: ProviderKind::Ollama,
    base_url: Some("http://localhost:11434".into()),
    api_key: None,                              // Ollama Local needs none
};

let provider = build_llm_provider(&cfg)?;       // Arc<dyn LlmProvider>
```

Cloud providers require an API key:

```rust
let cfg = ProviderConfig {
    kind: ProviderKind::Anthropic,              // or OpenAi / OpenRouter / OllamaCloud
    base_url: None,                             // default endpoint
    api_key: Some(std::env::var("ANTHROPIC_API_KEY")?),
};
```

API keys are AES-GCM-encrypted at rest in the `user_provider_configs` table (Phase 6). Auth headers are marked `set_sensitive(true)` so HTTP debug logs cannot leak them.

### Hardware tier → recommended model

| User hardware | Recommended local model |
|---------------|-------------------------|
| 8 GB RAM, no GPU | `qwen2.5-coder:7b` (Q4 quant, slow but works) |
| 16 GB RAM, no GPU | `qwen2.5-coder:7b` (Q4 / Q8) |
| 16 GB RAM + 8 GB VRAM (RTX 3060 / 4060) | `qwen2.5-coder:7b` or `deepseek-r1:7b` |
| 32 GB RAM + 12-16 GB VRAM (RTX 4070 Ti, M2 Pro) | `qwen2.5-coder:14b` |
| 32 GB RAM + 24 GB VRAM (RTX 4090, M3 Max 64 GB) | `qwen2.5-coder:32b` (near GPT-4 quality) |
| Apple M-series 16 GB+ | `qwen2.5-coder:7b` (MLX / Metal accelerated) |

Hardware detection runs on first launch and recommends the right tier.

---

## RAG pipeline

Phase 3 lands the producer chain that turns an uploaded folder into searchable chunks. Each stage owns a single file under `src/services/` or `src/repositories/`; service code never touches SQL or BLOB encoding (rules.md §4.2).

```
project folder
    │
    ▼
file_discovery_service::discover()
    │   gitignore-aware walk, extension allow-list,
    │   50 MiB / 500 MiB / 10 000-file caps,
    │   path-traversal + symlink-escape guards
    ▼
DiscoveredFile { relative_path, size, file_type, language }
    │
    ▼
ast_service::parse(source, SourceLanguage)
    │   Tree-sitter JS / TS / Python
    │   Declaration (Function/Method/Class),
    │   Import, Export, error_count
    ▼
ParsedFile
    │
    ▼
chunking_service::chunk_source(source, &ParsedFile)
    │   Splits at function / class boundaries,
    │   500–1 500 token target,
    │   oversize flag above 1 500
    ▼
Vec<Chunk>
    │   + EmbeddingProvider::embed(...)
    ▼
chunk_repo::insert_batch(pool, ChunkInsert[])
    │   BLOB-packed f32 vectors,
    │   per-(project, provider, dim) cap = 50 000,
    │   atomic transaction
    ▼
SQLite (code_chunks)
```

Search side:

```rust
use testing_ide_lib::repositories::chunk_repo::{search_similar, SEARCH_TOP_K_CAP};

let hits = search_similar(
    &pool,
    project_id,
    "ollama-nomic-embed-text",
    768,
    &query_embedding,
    SEARCH_TOP_K_CAP,
).await?;
```

`hits` are sorted by cosine similarity descending. The query path:

- Validates query length matches dimension (rejects cross-dim comparisons).
- Filters rows by `(project_id, embedding_provider, embedding_dim)` so results stay within one provider's vector space.
- Brute-force cosine for now; transparent migration to a `sqlite-vec` `vec0` virtual table when chunk count crosses 50 000 per tuple (see [ADR-0002](./apps/desktop/src-tauri/docs/adr/0002-vec0-migration-trigger.md)).
- Top-K clamped to `SEARCH_TOP_K_CAP` (50) regardless of caller request.

**Limits enforced server-side**:

| Limit | Source | Constant |
|-------|--------|----------|
| Per file | 50 MiB | `MAX_FILE_SIZE_BYTES` |
| Per project | 500 MiB | `MAX_PROJECT_SIZE_BYTES` |
| File count | 10 000 | `MAX_FILE_COUNT` |
| Chunk count per `(project, provider, dim)` | 50 000 | `MAX_CHUNKS_PER_TUPLE` |
| Search top-K | 50 | `SEARCH_TOP_K_CAP` |

---

## Project structure

```
Testing-IDE/
├── package.json                          # pnpm workspace root
├── pnpm-workspace.yaml
├── turbo.json
├── packages/                             # Shared workspace packages
│   ├── shared/                           # Zod schemas + TS types (FE/BE contract)
│   │   └── src/schemas/
│   │       ├── code-chunk.schema.ts      # Mirrors Rust ChunkKind
│   │       ├── llm-provider.schema.ts    # Mirrors Rust ProviderKind
│   │       ├── provider.schema.ts        # Mirrors user_provider_configs table
│   │       └── ...
│   ├── eslint-config/                    # Shared ESLint configs
│   ├── tsconfig/                         # Shared TS configs
│   └── ui/                               # Shared shadcn primitives (placeholder)
├── apps/
│   └── desktop/                          # Tauri 2 desktop app
│       ├── .env.example
│       ├── .gitignore
│       ├── package.json                  # Vite + React deps
│       ├── vite.config.ts
│       ├── components.json               # shadcn config
│       ├── src/                          # React frontend
│       │   ├── main.tsx
│       │   ├── App.tsx
│       │   ├── components/ui/button.tsx  # First shadcn primitive
│       │   ├── lib/utils.ts
│       │   └── index.css
│       └── src-tauri/                    # Rust backend
│           ├── Cargo.toml
│           ├── audit.toml                # cargo-audit triage
│           ├── build.rs
│           ├── capabilities/
│           ├── docs/adr/
│           │   ├── README.md                 # ADR index + frontmatter spec
│           │   ├── 0001-blob-embeddings.md
│           │   ├── 0002-vec0-migration-trigger.md
│           │   └── 0003-llm-provider-trait.md
│           ├── icons/
│           ├── migrations/
│           │   └── 0001_init.sql
│           ├── tauri.conf.json
│           └── src/
│               ├── main.rs
│               ├── lib.rs
│               ├── config.rs                 # Typed env loading
│               ├── error.rs                  # AppError + AppResult
│               ├── commands/                 # Tauri IPC handlers (Phase 6)
│               ├── services/                 # Business logic (Phase 3 — done)
│               │   ├── file_discovery_service.rs
│               │   ├── ast_service.rs
│               │   └── chunking_service.rs
│               ├── repositories/             # DB access (Phase 3 — done)
│               │   └── chunk_repo.rs         # BLOB insert + cosine search
│               ├── workers/                  # Background jobs
│               ├── prompts/                  # Versioned prompt templates (Phase 4)
│               ├── providers/                # External integrations (Phase 2 — done)
│               │   ├── factory.rs
│               │   ├── llm/
│               │   │   ├── mod.rs            # LlmProvider trait
│               │   │   ├── error.rs          # LlmError
│               │   │   ├── types.rs          # GenerateRequest, Chunk, etc.
│               │   │   ├── ollama.rs
│               │   │   ├── openai.rs
│               │   │   ├── openai_compat.rs  # Shared SSE parser
│               │   │   ├── openrouter.rs
│               │   │   └── anthropic.rs
│               │   └── embeddings/
│               │       ├── mod.rs            # EmbeddingProvider trait
│               │       └── ollama.rs
│               ├── db/                       # SQLite pool + migrations
│               └── utils/
│                   └── telemetry.rs          # tracing setup
├── plan/                                 # Planning docs
│   ├── initial-plan.md
│   ├── tech-stack.md
│   └── task-divide.md
├── rules/
│   └── rules.md                          # Engineering ruleset
└── README.md
```

---

## Engineering rules

All contributions — human or AI agent — must follow [`rules/rules.md`](./rules/rules.md). Key requirements enforced at PR review:

- **TypeScript strict mode** (no `any`, no non-null assertions, Zod at boundaries).
- **Rust** `#[deny(clippy::all)] + #[warn(clippy::pedantic)]`; no `unwrap()` / `expect()` in production paths.
- **No SDK leaks** in service code — providers always behind a trait.
- **Tests alongside source** (`foo.rs` + `foo_test.rs` or `#[cfg(test)] mod tests`).
- **Conventional Commits** with body explaining the *why*, citing rule sections by number.
- **No secrets in repo**, no string-concatenated SQL, no executing uploaded code.
- **Approved licenses only** — MIT / Apache-2.0 / BSD / ISC / MPL-2.0. GPL / AGPL / SSPL forbidden.

AI coding agents (Cursor, Claude Code, Copilot, Continue) **must read `rules/rules.md` before generating code in this repo** and cite rule numbers when explaining decisions.

---

## Architecture decisions

Significant decisions are captured as Architecture Decision Records under `apps/desktop/src-tauri/docs/adr/`:

| ADR | Title | Status |
|-----|-------|--------|
| 0001 | BLOB embeddings + brute-force cosine for MVP RAG | Accepted |
| 0002 | sqlite-vec vec0 migration trigger and rollout plan | Accepted |
| 0003 | LlmProvider trait shape and streaming model | Accepted |

See [`apps/desktop/src-tauri/docs/adr/README.md`](./apps/desktop/src-tauri/docs/adr/README.md) for the frontmatter convention and when-to-write-an-ADR guidance.

---

## Roadmap

### Shipped

- **Phase 1** — Foundation: monorepo + Tauri scaffold, layered architecture per [rules.md §4.2](./rules/rules.md), typed env config, AppError + AppResult, SQLite pool with WAL + foreign keys, migrations runner, schema for users / projects / files / chunks / artifacts / providers / dependencies, structured tracing logs.
- **Phase 2** — LLM provider abstraction: 4 chat providers + 1 embedding provider + factory + typed `LlmError`. 88 tests at end of phase, fmt + clippy + release build green, audit triaged.
- **Phase 3** — AST pipeline producer chain: `file_discovery_service` (gitignore-aware walk, extension allow-list, size caps, path-traversal guards), `ast_service` (Tree-sitter JS/TS/Python with declaration / import / export extraction), `chunking_service` (function / class / module-boundary chunks with token counts + oversize flag), `chunk_repo` (BLOB embeddings with brute-force cosine search filtered by `(project_id, embedding_provider, embedding_dim)`, top-K capped at 50, 50 000-chunk-per-tuple cap per ADR-0002). 136 tests at end of phase.
- **Monorepo + frontend scaffold** (parallel stream) — pnpm workspaces, Turborepo, shared Zod schemas mirroring Rust types per `rules.md` §12.3.1, ESLint / TS configs, Vite + React 19 + Tailwind + shadcn skeleton. First Tauri IPC commands wired (`greet`, `init_db`).

### Next

- **Phase 4** — Versioned prompt templates (`prompts/{context, test_plan, test_cases, defect_report}_v1.rs`) with JSON-Schema tool-calling for structured output. Snapshot tests via `insta`.
- **Phase 5** — Generation service tying RAG + prompts + LLM streaming. Token-budget enforcement (`AppError::LimitExceeded`).
- **Phase 6** — Tauri IPC commands (`projects`, `analysis`, `generation`, `providers`, `health`). AES-GCM API-key encryption helper. First-run wizard, hardware detection.
- **Phase 7** — Integration tests against Ollama (no API credit needed), full test suite via `cargo llvm-cov`, GitHub Actions CI matrix (Windows / macOS / Linux), release-bundle workflow via `tauri-action`.

### Beyond

- Frontend: React 19 + Monaco + shadcn/ui (Student 1 stream).
- Multi-language AST support: Java, Go, Rust, C# (Phase 3.5).
- Export formats: PDF (Puppeteer), JIRA ADF, custom JSON schema (V1).
- Marketing + docs site: Next.js 15 on Vercel (separate workstream).
- Self-hosted enterprise option: vLLM / SGLang serving open-source models behind the same `LlmProvider` trait.

---

## Contributing

1. Read [`rules/rules.md`](./rules/rules.md) end to end before opening a PR.
2. Branch naming: `feat/<scope>/<short-desc>`, `fix/<scope>/<short-desc>`, `chore/<scope>/<short-desc>`.
3. Conventional Commits with bodies that explain the *why* and cite rules sections.
4. Every PR must pass `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release --lib` before review.
5. New deps justified per `rules.md` §11 (license check, maintenance status, alternatives considered).
6. Snapshot tests for any LLM prompt output; mockito (or equivalent) for HTTP boundaries — no live API calls in `cargo test`.

Pre-merge audit prompt and per-phase implementation prompts are in `plan/`.

---

## Security

- **No execution of uploaded code** — the analysis pipeline parses source as text via Tree-sitter. Static analysis only.
- **Secrets**: API keys encrypted at rest (AES-GCM, Phase 6). Auth headers marked `HeaderValue::set_sensitive(true)` so `tracing` debug logs cannot leak them. Regex-based pre-LLM scanning redacts API-key patterns from any code shipped to a model.
- **File uploads**: extension allowlist (no blacklists), magic-byte validation, 50 MB / file, 500 MB / project, 10 000-file caps. Path-traversal prevention.
- **Network**: HTTPS-only, rustls TLS, CORS allowlist, Helmet equivalents. No telemetry to third parties.
- **Dep advisories**: `cargo audit` with `audit.toml` triage; high / critical CVEs block merge per [rules.md §11](./rules/rules.md).

Report security issues privately to the maintainer rather than opening a public issue.

---

## License

License pending. Treat as **All Rights Reserved** until a final license is committed.

---

## Acknowledgements

- [Tauri](https://tauri.app/) for the secure, lightweight desktop shell.
- [tree-sitter](https://tree-sitter.github.io/tree-sitter/) for incremental, language-agnostic parsing.
- [Ollama](https://ollama.com/) for making local LLM inference frictionless.
- [Anthropic](https://anthropic.com/), [OpenAI](https://openai.com/), [OpenRouter](https://openrouter.ai/) for the cloud providers wired in via the same trait.
