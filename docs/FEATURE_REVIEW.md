# Tessera — Feature Review & Roadmap

> Reviewed: 2026-06-04 | Version: v0.1 | Codebase: ~26K LOC (15.6K Rust + 8K React + 2.4K shared)

A feature-by-feature scorecard of what ships today, where each capability is strong or thin, the concrete improvements that move the needle, and the next features that take Tessera from a solid v0.1 foundation to a production-grade product.

---

## At a Glance

Tessera is a **local-first AI testing IDE**: open a folder, and it runs tree-sitter AST analysis + RAG over your code, then calls an LLM (Ollama by default; OpenAI / Anthropic / OpenRouter optional) to generate five reviewable QA artifacts — Context, Test Plan, Test Cases, Defect Report, Bug Report. No code execution, no source upload on the default path.

**Verdict: ~4.1 / 5.** The engineering foundation is production-grade — clean layering, trait-based providers, typed errors, AES-GCM key storage, 339+ tests, an exemplary CI/CD gauntlet. What's still MVP is *breadth*: vector search is brute-force, the editor is read-only, RAG is locked to one embedding provider, and artifacts can't leave the local DB except as a Markdown file.

### Reality check (claims vs. code)

Three places where the README / earlier audit run ahead of the implementation. Worth correcting before they reach a user:

| Claim | Reality | Where |
|---|---|---|
| "SQLite + `sqlite-vec` / vec0 index" | Vector search is **brute-force cosine over BLOB `f32` vectors**; sqlite-vec is **deferred** until 50K chunks (ADR-0002). | `repositories/chunk_repo.rs`, `services/generation_service.rs` |
| Monaco "editor" implies editing | Editor is **read-only** — in-memory edits + dirty flag are wired, but there is **no save action** (deferred Phase 11). | `components/editor/editor-panel.tsx`, `stores/editor-store.ts` |
| Full JWT / Argon2 / refresh auth | Fully built and timing-safe, but **vestigial** — one seeded local user, `DEFAULT_USER_ID` threaded everywhere. Right code, wrong scope for a single-user desktop app. | `auth/`, `services/auth_service.rs` |

---

## Codebase Statistics

| Component | Files | Lines | Purpose |
|-----------|-------|-------|---------|
| Rust backend | 68 | 15,655 | Commands → services → repositories → SQLite |
| React frontend | 69 | 8,096 | Components, Zustand stores, typed IPC client |
| Shared (Zod) | 30+ | 2,377 | Schemas + inferred TS types (the FE/BE contract) |
| Tests | 14 | 1,979 | Unit + integration + E2E |
| Migrations | 3 | 281 | Schema evolution |
| CI/CD | 3 | ~350 | GitHub Actions workflows |

### Test coverage

| Category | Count |
|----------|-------|
| Rust unit tests | 218 |
| TypeScript unit tests | 43 |
| Zod schema tests | 78 |
| Integration tests (live Ollama) | 2 |
| E2E tests (Playwright) | 1 |
| Snapshot tests (Insta) | 6 |
| **Total** | **339+** |

---

## Feature Scorecard

Each shipped feature rated out of 5, with the reason and the gap.

| # | Feature | Rating | Why / Gap |
|---|---------|:---:|-----------|
| 1 | Artifact generation (5 types, RAG + JSON-Schema) | **4.5** | Core flow solid; schema-validated with a salvage path for non-tool-trained models. No retry, no cancel. |
| 2 | LLM provider abstraction (5 providers) | **4.5** | Clean async trait + factory; per-provider streaming; Anthropic on its native wire format. |
| 3 | Streaming + partial-JSON preview | **4.5** | State-machine preview from incomplete JSON, blinking caret. Polished. No mid-stream cancel. |
| 4 | Prompt versioning + JSON-Schema tool calls | **4.5** | Per-prompt `VERSION` const, insta snapshots, `jsonschema` validation of output. |
| 5 | AES-GCM key encryption + provider config | **4.5** | Keys encrypted at rest, never returned, nonce stored. Key derived from `JWT_SECRET` (single point of weakness). |
| 6 | AST analysis (tree-sitter) | **4.0** | Error-tolerant extraction of decls/imports/exports. Only JS/TS/Python; no call graph. |
| 7 | Semantic chunking | **4.0** | Boundary-aware, 500–1500 tokens, oversize flag. Method-in-class duplication bloats the index. |
| 8 | AI panel / review queue / lifecycle | **4.5** | Generate, stream, approve/reject, regenerate-with-feedback. Polished UX. |
| 9 | Artifact versioning + diff view | **4.5** | `parent_id` chain, dependency-free LCS line diff, version compare dropdown. |
| 10 | Command palette + shortcuts + command bus | **4.5** | 13 commands, full keyboard nav, idempotent menu/shortcut dispatch. VS Code-grade. |
| 11 | First-run wizard / onboarding | **4.5** | 4 steps with a real hardware probe, Ollama connectivity test, model-pull check. |
| 12 | File explorer | **4.0** | Lazy `react-arborist` tree, keyboard nav. No search / filter. |
| 13 | RAG retrieval pipeline | **3.5** | Works, but brute-force cosine + Ollama-only embeddings cap both scale and quality. |
| 14 | Vector storage | **3.0** | BLOB `f32`, linear scan, 50K cap. sqlite-vec promised but not shipped. |
| 15 | Markdown export | **3.5** | Disk save via Tauri dialog. No JIRA / Linear / GitHub connectors. |
| 16 | Monaco editor | **3.0** | Real editor, syntax highlighting, tabs, dirty tracking — but read-only, no save. |
| 17 | Hardware tier detection | **3.5** | RAM → model-tier mapping from a real probe. No GPU / VRAM detection — the metric that actually drives LLM fit. |
| 18 | Auth (JWT / Argon2 / refresh) | **3.0** | Well-built and timing-safe, but unused in the single-user reality. Right code, wrong scope. |
| 19 | CI/CD + guards | **5.0** | 5-gate CI, pre-push gauntlet, Husky hooks, cross-platform release, branch protection. Exemplary. |
| 20 | Test suite | **4.0** | 339+ unit / contract / snapshot + live Ollama integration. E2E thin (1 spec), no coverage reporting. |
| 21 | Security posture | **4.5** | Path-traversal guards, size caps, parameterized SQL, XSS-safe Markdown, encrypted keys. |
| 22 | Theming / accessibility | **4.0** | Dark mode, 84+ aria attributes, focus management. No automated a11y gate. |

**Weighted overall: ~4.1 / 5** — a production-quality foundation with MVP breadth.

---

## Improvements to Current Features

Ordered by leverage.

### RAG & vector search — the biggest single lever
- **Ship sqlite-vec (vec0) now**, not at the 50K-chunk trigger. Brute-force is already O(n) per query, and shipping it closes the README/code gap.
- **Drop method-in-class chunk duplication** (or dedupe at retrieval). Methods are currently emitted both inside their class chunk *and* individually — roughly halving index size and cutting retrieval noise.
- **Add cloud embedding providers** (OpenAI / Voyage / Cohere) behind the existing `EmbeddingProvider` trait. Rows are already keyed by `embedding_provider` + `dim`, so the storage layer is ready. Unlocks usable RAG for users without a local GPU.

### Generation robustness
- **Cancellation token.** Long Ollama cold starts (600s timeout) cannot currently be aborted. Thread a `CancellationToken` through `generate()` and the stream loop, wired to a Stop button.
- **Retry with backoff** on transient 429 / 5xx. Anthropic and OpenAI already return `Retry-After` (parsed in tests) — honor it with bounded exponential backoff.
- **Per-provider token counting.** The 4-chars-per-token heuristic over- or under-budgets real models. Use tiktoken for OpenAI and the Anthropic count-tokens API.

### Editor
- **Finish save** (Ctrl+S → `fs.writeTextFile`, clear dirty). The dirty flag is already wired; a half-built, user-visible feature is a trust cost.
- **File-tree search + fuzzy open.** The command-palette infrastructure already exists; extend it to files.

### Hardware tier
- **Detect GPU / VRAM** via an nvml/wgpu Tauri command. RAM alone mis-recommends models on GPU machines.

### Auth
- **Decide its scope.** Either gate it behind a "team / hosted" feature flag, or strip it from the desktop path. Dead-but-shipped auth is an audit surface and a reviewer trap.

### Observability
- **LCOV in CI** (`cargo-llvm-cov` + Vitest coverage) with a threshold gate. CLAUDE.md cites an 80% target that nothing currently measures.
- **Tracing spans** around generation latency (embed / RAG / LLM breakdown) to surface the slow stage.

### E2E
- **Expand from 1 spec to ~10–15** covering the generation flow, provider switching, error states, and export — the highest-risk UI paths are currently untested.

---

## Five Features for Production-Grade

Ranked by differentiation × fit with existing infrastructure. (#1, #2, #5 echo earlier roadmap thinking — they remain the right calls; #3, #4 are sharper additions.)

### 1. Closed-loop sandboxed test runner + coverage overlay — flagship
Generate Test Cases → execute them in a per-language sandbox (Docker / Wasm) → paint pass/fail and line coverage onto the Monaco gutters. No AI testing tool closes the generate → run → measure loop. Keep the sandbox opt-in so the local-first, no-execution guarantee holds on the default path. **Turns a static generator into objective proof.**

### 2. Test ↔ code traceability graph + stale-test detection
The `dependencies` table already exists in the schema but is **never populated** — build the call graph from the tree-sitter AST that's already in place. Link each generated test case to its source symbol. On a git diff (file watcher or pre-commit hook), flag tests that reference changed functions as "stale" and regenerate only those. Combines impact-graph + diff-aware incremental generation, visualized as a force-directed graph. **Makes Tessera test *intelligence*, not one-shot generation.**

### 3. Prompt Studio + eval harness
User-editable, versioned prompt templates with variable substitution, plus a golden-set regression scoreboard. The insta snapshots and the `express-api` golden fixture already exist — promote them into a runnable eval that scores artifact quality per prompt-version per model and catches regressions before they ship. **This is the LLM-ops discipline that separates a maintainable product from a demo.**

### 4. Export / sync connectors (JIRA / Linear / GitHub Issues)
Artifacts are currently trapped in local SQLite. Add export adapters that push Defect and Bug reports as tickets and round-trip status back. The structured-output design already makes artifacts cleanly mappable. **This is the adoption unlock for real QA teams — Markdown export alone isn't workflow-integrated.**

### 5. Multi-model consensus panel
Run one prompt across 2–3 providers concurrently (Ollama + OpenAI + Anthropic — the trait already supports it), show artifacts side by side, and highlight disagreement (a strong signal for edge cases that deserve human attention). Let the reviewer cherry-pick the best sections from each. **Cheap to build on the existing provider trait + streaming, high perceived value.**

> **Hardening note:** shipping real sqlite-vec ANN + cloud embeddings (Improvements → RAG) is arguably more urgent than any feature above for product credibility — it closes the gap between what the README promises and what the code does.

---

## Quality Scorecard

| Dimension | Grade | Detail |
|-----------|:---:|--------|
| Architecture | A+ | Layered (commands → services → repos), trait-based providers, factory pattern |
| Type safety | A+ | Strict TypeScript, full Rust safety, Zod at every boundary |
| Error handling | A+ | Typed errors with stable codes, graceful degradation |
| Testing | A− | 339+ tests, live integration; thin E2E, no coverage reporting |
| Documentation | A− | Excellent `rules.md` + ADRs; README overstates vector search & editor |
| Security | A+ | AES-GCM, Argon2, JWT, parameterized SQL, local-first default |
| CI/CD | A+ | Multi-stage guards, cross-platform release, branch protection |
| Accessibility | B+ | 84+ aria attributes; no automated audit in CI |
| Code cleanliness | A+ | No TODO/FIXME in production, Clippy pedantic clean |
| Onboarding | A | First-run wizard, env docs, clear quickstart |
