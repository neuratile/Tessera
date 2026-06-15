# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Tessera — local-first AI testing IDE. Generates test artifacts (Context, Test Plan, Test Cases, Defect Report, Bug Report) by running static analysis (tree-sitter AST) + RAG over local code, then calling an LLM (Ollama default; also OpenAI, Anthropic, Google Gemini, OpenRouter). Embeddings are selected independently of the LLM (local Ollama default; OpenAI / Gemini / Hugging Face cloud optional — see `plan/versions/v1/EMBEDDING_PROVIDER_SELECT.md`; `embedding_config_service::resolve_provider` is the only production path that constructs an `EmbeddingProvider`). Static analysis only on the default path — no remote code upload (cloud embeddings, when explicitly selected, send code snippets to that provider). An **opt-in** local Docker sandbox (off by default) executes generated JS/TS and Python test cases to report pass/fail + line coverage; it runs with no network and the backend rejects runs unless opt-in is confirmed. See `plan/versions/v1/SANDBOX_TEST_RUNNER.md` (JS/TS), `plan/versions/v1/SANDBOX_PYTHON_RUNNER.md` (Python) + `apps/desktop/src-tauri/docs/adr/0004-sandbox-test-runner.md`.

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
pnpm guard:pre-push

# Quick conflict-marker scan only
pnpm guard:markers
```

## Architecture

### Monorepo layout

```
apps/desktop/        Tauri shell — React frontend + Rust backend
  src/               React app (components/, stores/, lib/ipc/)
  src-tauri/src/     Rust library crate

packages/
  shared/            Zod schemas + inferred TS types — the FE/BE contract
  eslint-config/     Shared ESLint presets
  tsconfig/          Shared TS configs

rules/rules.md       Canonical engineering rulebook (§1–12) — read before adding patterns
docs/AGENT_WORKFLOW.md  PR/branch/CI contract
plan/                Roadmap (ROADMAP.md) + design docs for multi-day work,
                     grouped by release under plan/versions/ (v1/, v2/ — see
                     plan/versions/README.md)
```

### Rust backend layering (commands → services → repositories → db)

```
commands/   Tauri IPC handlers — thin, validate input, delegate immediately
services/   Business logic — no SQL, no Tauri types
repositories/  SQL only — no business logic
db/         Schema init, migrations
providers/  LLM + embedding trait impls (pluggable at runtime via factory.rs);
            providers/runners/ holds the TestRunner trait, the shared Docker
            hardening harness (docker_harness.rs), the docker_js + docker_py
            sandbox impls, and per-language selection (runners/factory.rs)
prompts/    Versioned prompt templates with JSON-Schema tool definitions
auth/       JWT + Argon2 password + AES-256-GCM API key encryption
utils/      Pure functions (crypto, telemetry, path helpers)
```

`generation_service.rs` is the sole entry point for LLM calls (per §4.2 + §12.1). Flow: embed scope hint → RAG via `chunk_repo::search_similar` → `build_prompt` → token budget check → `LlmProvider::stream` → JSON-Schema validate tool output → `artifact_repo::insert`.

`sandbox_service.rs` is the sole entry point for test execution (mirrors the generation-service pattern; commands in `commands/sandbox.rs`). Flow: load source + generated test `files[]` off the test-cases artifact → detect language from file extensions (mixed Python + JS/TS workspaces rejected) → `RunInput::validate` (path-traversal + file-count/byte guards) → select the runner via `runners::factory::runner_for` (JS/TS → `docker-js`, Python → `docker-py`) → `TestRunner::run` inside a hardened, network-less Docker container (flags emitted only by `docker_harness::hardened_run_args`, asserted by a drift-tripwire unit test: `--network none`, `--cap-drop ALL`, non-root, read-only rootfs, cpu/mem/pids/fsize caps, wall-clock timeout + cancel → `docker kill`) → parse vitest + istanbul JSON (JS) or pytest-json-report + coverage.py JSON (Python) → `test_run_repo` batch insert → return `RunResult`. Opt-in, off by default — backend rejects a run when `optInConfirmed` is false. Data model: migration `0004_test_runs.sql` (`runner` column is open TEXT — `docker-py` needed no migration). Threat model + security gate: ADR-0004. `sandbox_service::run_flaky` is a second entry point for **flaky-test detection** (`plan/versions/v2/v2-feature-docs/FLAKY_TEST_DETECTION.md`): it shares `run`'s preamble, loops the same runner N times (clamped 2–20) under one `CancelToken`, persists iteration #1 via the normal path, and classifies each test stable-pass / stable-fail / flaky via the pure `aggregate_flaky`. A completed check is also persisted as **flaky history** (migration `0008_flaky_checks.sql`: additive `flaky_checks` + `flaky_check_tests`; `run_id` FK is `ON DELETE SET NULL` so history outlives a purged run) via `flaky_check_repo` — best-effort, so a history-write failure never discards the in-memory result. `sandbox_service::{list_flaky_history, get_flaky_check}` (commands `list_flaky_checks` / `get_flaky_check`) read it back for the "Flaky history" trend in the sandbox panel.

### Tauri IPC gotchas (rules.md §4.2.1)

- `#[tauri::command]` requires **owned argument types** (`String`, not `&str`). This trips `clippy::needless_pass_by_value` — silence it at the command function with a comment, never globally.
- Commands return `Result<T, String>` (Tauri serializes the error variant). Map domain errors with `.map_err(|e| e.to_string())` at the boundary; keep typed `AppError` everywhere inside.
- `app.manage(...)` / `handle.path()` require `use tauri::Manager;` — forgetting it is a silent rust-analyzer suggestion that fails to compile.

### FE/BE schema sync (rules.md §12.3.1)

Rust serde is the source of truth. TS Zod schemas in `packages/shared/` mirror it — they never drive it.

- `#[serde(rename = "ollama")]` → `z.literal('ollama')` — discriminator strings must match exactly.
- `Option<T>` → `.optional()` in Zod; encode Rust refinements as `.refine(...)`.
- When a Rust enum gains or drops a variant, update the Zod schema and all covering tests in the **same PR**.
- New schemas go in `packages/shared/src/schemas/` with a round-trip contract test.

### Provider abstraction

`LlmProvider` and `EmbeddingProvider` are async traits. Implementations live under `providers/llm/` (ollama, openai, anthropic, openrouter, openai_compat) and `providers/embeddings/`. `providers/factory.rs` selects implementation at runtime from `provider_config_repo`. Never call provider code directly outside `generation_service`.

### Frontend state

Zustand stores in `src/stores/`. Tauri IPC calls go through typed wrappers in `src/lib/ipc/` — all payloads validated with Zod against types from `@testing-ide/shared`. No raw `invoke()` calls outside `ipc/`.

### Shared types

`packages/shared/` is the single source of truth for types crossing the IPC boundary. Add Zod schemas there first; TS types are inferred from them.

## Branching

`<type>/<short-slug>` — kebab-case, ≤ 40 chars. Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `chore`, `test`.
Examples: `feat/streaming-preview`, `fix/ollama-404-hint`, `chore/upgrade-tauri-2.6`.

Husky wires automatically on `pnpm install`: `git commit` runs conflict-marker + large-file (> 5 MB) check; `git push` runs the full gauntlet.

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
