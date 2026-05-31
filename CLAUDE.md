# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Tessera — local-first AI testing IDE. Generates test artifacts (Context, Test Plan, Test Cases, Defect Report, Bug Report) by running static analysis (tree-sitter AST) + RAG over local code, then calling an LLM (Ollama default; also OpenAI, Anthropic, OpenRouter). No code execution. No remote code upload.

Stack: React 19 + TypeScript + Tailwind v4 (Vite) inside a Tauri 2.0 shell, Rust backend with SQLite + sqlite-vec, tree-sitter for JS/TS/Python.

## Commands

All commands run from repo root unless noted.

```bash
# Install deps (once)
corepack pnpm install

# Full dev (Vite + Rust backend hot-reload)
pnpm --filter @testing-ide/desktop run dev

# Build production bundle
pnpm build

# Typecheck all packages
pnpm typecheck

# Lint all packages (ESLint + Clippy in CI)
pnpm lint

# Run all tests (frontend Vitest + Rust cargo test --lib)
pnpm test

# Frontend tests only
pnpm --filter @testing-ide/desktop run test:frontend

# Rust unit tests only
pnpm --filter @testing-ide/desktop run test:rust
# equivalent: cargo test --lib --manifest-path apps/desktop/src-tauri/Cargo.toml

# Single Rust test (by name filter)
cargo test --lib --manifest-path apps/desktop/src-tauri/Cargo.toml <test_name>

# Integration tests (require live Ollama on :11434)
pnpm --filter @testing-ide/desktop run test:integration

# Playwright E2E
pnpm --filter @testing-ide/desktop run test:e2e

# Clippy (matches CI flags exactly)
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --all-targets --lib -- -D warnings

# Pre-push local CI gauntlet (typecheck → lint → test → clippy)
bash tools/scripts/pre-push.sh
```

## Architecture

### Monorepo layout

```
apps/desktop/        Tauri shell — React frontend + Rust backend
  src/               React app (components/, stores/, lib/ipc/)
  src-tauri/src/     Rust library crate

packages/
  shared/            Zod schemas + inferred TS types — the FE/BE contract
  ui/                shadcn/ui component library
  eslint-config/     Shared ESLint presets
  tsconfig/          Shared TS configs

rules/rules.md       Canonical engineering rulebook (§1–12) — read before adding patterns
docs/AGENT_WORKFLOW.md  PR/branch/CI contract
plan/                Roadmap (ROADMAP.md) + design docs for multi-day work
```

### Rust backend layering (commands → services → repositories → db)

```
commands/   Tauri IPC handlers — thin, validate input, delegate immediately
services/   Business logic — no SQL, no Tauri types
repositories/  SQL only — no business logic
db/         Schema init, migrations
providers/  LLM + embedding trait impls (pluggable at runtime via factory.rs)
prompts/    Versioned prompt templates with JSON-Schema tool definitions
auth/       JWT + Argon2 password + AES-256-GCM API key encryption
utils/      Pure functions (crypto, telemetry, path helpers)
```

`generation_service.rs` is the sole entry point for LLM calls (per §4.2 + §12.1). Flow: embed scope hint → RAG via `chunk_repo::search_similar` → `build_prompt` → token budget check → `LlmProvider::stream` → JSON-Schema validate tool output → `artifact_repo::insert`.

### Provider abstraction

`LlmProvider` and `EmbeddingProvider` are async traits. Implementations live under `providers/llm/` (ollama, openai, anthropic, openrouter, openai_compat) and `providers/embeddings/`. `providers/factory.rs` selects implementation at runtime from `provider_config_repo`. Never call provider code directly outside `generation_service`.

### Frontend state

Zustand stores in `src/stores/`. Tauri IPC calls go through typed wrappers in `src/lib/ipc/` — all payloads validated with Zod against types from `@testing-ide/shared`. No raw `invoke()` calls outside `ipc/`.

### Shared types

`packages/shared/` is the single source of truth for types crossing the IPC boundary. Add Zod schemas there first; TS types are inferred from them.

## Key Rules (from rules/rules.md)

- **No `any`, no non-null assertions, no `as` casts** in TypeScript. Zod at all IPC boundaries.
- **No `unwrap()`/`expect()` in production Rust** outside `main.rs` setup. Use `?` + `thiserror`.
- **Clippy pedantic** is enforced. Lint locally before pushing.
- **Conventional commits** required: `type(scope): description`. Types: feat, fix, refactor, test, docs, chore, ci.
- **master is always green + linear**. Squash merge only. No merge commits.
- **Prompts are versioned** (`VERSION` const in each prompt module). Schema-validate all LLM output via `jsonschema`.
- **Stream LLM responses** — never buffer full response before forwarding to UI.
- **No N+1 queries** — batch or join. Paginate all list endpoints.
- **Parameterized SQL only** — `sqlx::query!` macros preferred.
- **No `console.log`** in frontend — use `tracing` on Rust side; structured logs only.

## Testing Conventions

- Rust: `#[cfg(test)]` modules in same file as the code under test. Mock providers via trait objects (`ScriptedLlm`, `ScriptedEmbeddings` pattern in `generation_service.rs` tests).
- Frontend: Vitest, `vitest.config.ts` for unit, `vitest.integration.config.ts` for live-Ollama tests.
- Snapshot tests for prompts: `insta` crate, snapshots in `prompts/snapshots/`.
- 80% line coverage target on services and utilities; UI components exempt.

## Environment

Copy `.env.example` → `.env` at repo root. Desktop app also reads `apps/desktop/.env`.
Minimum for local dev: `OLLAMA_BASE_URL=http://localhost:11434`. JWT_SECRET optional locally.
Run `pnpm bootstrap:ollama` to pull required models after installing Ollama.

## Release

Tag a commit: `git tag v0.x.y && git push origin v0.x.y`. CI matrix builds signed bundles for Windows / macOS / Linux via `tauri-apps/tauri-action` and publishes to GitHub Releases.
