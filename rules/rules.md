# Engineering Rules — Testing IDE

> **Purpose**: This is the canonical ruleset for all code contributions to this repository, including code written by AI coding agents (Cursor, Claude Code, GitHub Copilot, Cody, Continue, etc.).
>
> **Audience**: Human developers AND LLM coding assistants. AI agents must read this file before generating any code in this repo. Reference it explicitly in your context.
>
> **Authority**: These rules override personal preferences. Violations are blocked at PR review. Style is not optional.

---

## 1. Core Principles

1. **Clarity over cleverness** — code is read 10x more than it's written
2. **Boring is good** — pick the most obvious, idiomatic solution
3. **Explicit over implicit** — types, names, and intent should be obvious from the code
4. **Small surface area** — every module should have one reason to change
5. **No magic** — avoid metaprogramming, decorators, runtime monkey-patching unless absolutely required
6. **Test the contract, not the implementation** — tests should survive refactors
7. **Fail loudly, fail early** — runtime errors should crash with useful messages, not be silently swallowed
8. **Local-first** — code must work without external API access (Ollama default for LLM, SQLite local for DB)

---

## 2. Language Rules

### 2.1 TypeScript (frontend + Node.js)

**Required compiler settings (non-negotiable):**

```json
{
  "strict": true,
  "noUncheckedIndexedAccess": true,
  "noImplicitOverride": true,
  "noFallthroughCasesInSwitch": true,
  "exactOptionalPropertyTypes": true,
  "noUnusedLocals": true,
  "noUnusedParameters": true,
  "forceConsistentCasingInFileNames": true,
  "skipLibCheck": true
}
```

**Rules:**

- **No `any`** — use `unknown` and narrow with type guards. If `any` is unavoidable, comment why.
- **No type assertions (`as Foo`) without justification** — prefer type guards or schema validation (Zod).
- **No non-null assertions (`!`)** — use early returns or proper null handling.
- **No `// @ts-ignore` or `// @ts-expect-error`** without a comment explaining why and a TODO with a ticket reference.
- **Prefer `type` over `interface`** unless declaration merging is needed.
- **Use Zod schemas** for any data crossing a trust boundary (API requests/responses, DB inputs, env vars, user input).
- **Use discriminated unions** for state machines (`{ status: 'idle' } | { status: 'loading' } | { status: 'error', error: Error }`).
- **No `enum`** — use `as const` objects or string literal unions.
- **All async functions return `Promise<T>`** explicitly typed; never `Promise<any>`.
- **No floating promises** — every promise must be awaited or explicitly chained with `.catch()`. Enforced by `@typescript-eslint/no-floating-promises`.

### 2.2 Rust (Tauri backend)

**Required:**

- `#![deny(clippy::all)]` and `#![warn(clippy::pedantic)]` at crate root
- `rustfmt.toml` with project defaults — no inline overrides
- Use `Result<T, E>` for fallible operations; `panic!` only for invariant violations
- Use `thiserror` for library errors, `anyhow` for application errors
- **No `unwrap()` or `expect()` in production code paths** — only in tests or one-time startup that is guaranteed safe (and document why)
- Prefer `&str` over `String` for function parameters
- Use `tracing` for logs, not `println!`
- All public functions and structs require doc comments (`///`)
- No `unsafe` blocks without team review and a `// SAFETY:` comment explaining invariants

### 2.3 SQL

- **All schema changes via migrations** — never edit DB directly
- Use parameterized queries — never string-concatenate user input into SQL
- Prefer Drizzle/sqlx query builder over raw SQL; raw SQL allowed when query builder is too limiting, with a comment explaining why
- Index every foreign key and every column used in `WHERE` or `ORDER BY`
- Use snake_case for table and column names
- Every table has `id`, `created_at`, `updated_at` columns

---

## 3. Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Files (TS/TSX) | `kebab-case.ts` | `user-service.ts` |
| Files (React components) | `PascalCase.tsx` | `FileTree.tsx` |
| Files (Rust) | `snake_case.rs` | `llm_provider.rs` |
| Folders | `kebab-case` | `ai-pipeline/` |
| Variables / functions | `camelCase` | `parseFile()` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_FILE_SIZE` |
| Types / Interfaces / Classes | `PascalCase` | `ProjectFile` |
| React components | `PascalCase` | `<FileTreeNode />` |
| React hooks | `useCamelCase` | `useProjectAnalysis` |
| Boolean variables | `is/has/can/should + Adjective` | `isLoading`, `hasError` |
| Event handlers | `handle + Event` (define) / `on + Event` (prop) | `handleClick` / `onClick` |
| Async functions returning data | verb-led, no `Async` suffix | `fetchProjects()` not `getProjectsAsync()` |
| DB tables | `snake_case` plural | `project_files` |
| Env vars | `UPPER_SNAKE_CASE`, prefixed | `OLLAMA_BASE_URL` |
| Routes | `/api/kebab-case/resource` | `/api/projects/:id/generate` |
| Branch names | `feat/<scope>/<short-desc>` | `feat/ai-pipeline/rag-indexing` |

---

## 4. File / Folder Structure

### 4.1 Universal rules

- **One module = one responsibility.** If a file exceeds ~300 lines, split it.
- **No barrel files (`index.ts` re-exporting everything)** at package root unless required for public API. Internal barrels OK if scoped.
- **Co-locate tests** with source: `user-service.ts` next to `user-service.test.ts`.
- **Co-locate types** with the module that owns them. Export shared types from `packages/shared/`.
- **No deep relative imports (`../../../foo`)** — configure path aliases (`@/services/foo`).

### 4.2 Backend layered architecture

```
src/
  routes/              -- HTTP route handlers (thin, validation only)
  services/            -- Business logic (testable, no HTTP awareness)
  repositories/        -- Database access (no business logic)
  workers/             -- Background jobs
  providers/           -- External integrations (LLM, embeddings)
  utils/               -- Pure functions, no side effects
  middleware/          -- Express middleware
  db/
    schema.ts          -- Drizzle schema
    migrations/        -- Generated migrations
  config/              -- Env loading, typed config
  types/               -- Internal types not in packages/shared
```

**Layering rules:**
- Routes → Services → Repositories → DB
- Services may call Providers (LLM, etc.)
- **Never** call routes from services. **Never** import a higher layer from a lower one.
- **No business logic in routes** — routes parse input, call service, format response
- **No SQL in services** — all DB access goes through repositories

### 4.3 Frontend structure

```
src/
  components/
    ui/                -- shadcn primitives (Button, Input, etc.)
    features/          -- Feature-scoped components (file-tree, ai-panel)
    layout/            -- App shell, panels, toolbar
  hooks/               -- Custom React hooks
  stores/              -- Zustand stores
  lib/
    api.ts             -- API client
    utils.ts           -- Pure utilities
  pages/               -- Route components
  styles/              -- Global CSS
```

---

## 5. Architecture Patterns

### 5.1 Dependency Injection

- Services accept dependencies via constructor / factory parameters, not module-level imports
- Enables mocking in tests
- Example: `LlmService(provider: LlmProvider)` not `import { openaiClient } from './openai'`

### 5.2 Provider Abstraction (LLM, Embeddings)

- All external services behind an interface (`LlmProvider`, `EmbeddingProvider`)
- Concrete implementations: `OllamaProvider`, `OpenAIProvider`, `AnthropicProvider`, `OpenRouterProvider`
- Service code never references a specific provider; selects via factory at runtime

### 5.3 Error Handling

- **Typed errors** — every error has a `code` field + `message`. Use `thiserror` (Rust) or custom error classes (TS).
- **No throwing strings** — always throw `Error` instances or subclasses.
- **Catch only what you can handle** — don't catch-and-rethrow without adding context.
- **Errors propagate upward** — handle at the highest layer that knows how to respond (route handler maps to HTTP status).
- **User-facing messages** must not leak internals (no stack traces, no SQL errors, no file paths).

### 5.4 Logging

- Use `tracing` (Rust) / `pino` (Node) — structured JSON logs only
- **No `console.log` in committed code.** Use logger.
- Log levels: `error` (failure), `warn` (degraded), `info` (lifecycle events), `debug` (dev only)
- **Never log secrets** — API keys, passwords, tokens, PII
- Include correlation ID on every request (UUID) for tracing across services

---

## 6. Testing Rules

- **Every public function in a service or utility has a unit test.**
- **Every API endpoint has an integration test.**
- **Critical user flows have an E2E test** (Playwright).
- Tests live next to source: `foo.ts` + `foo.test.ts`.
- **Test naming**: `describe('FunctionName', () => { it('should do X when Y', ...) })`.
- **Arrange-Act-Assert** pattern. Comments separating sections optional but encouraged.
- **No shared mutable state between tests.** Each test isolated.
- **Use `beforeEach` to set up, `afterEach` to clean up.** No leaking state.
- **Mock at the boundary** — mock the LLM provider, not internal functions.
- **For LLM output tests**: validate against JSON Schema (Zod), not exact string matches.
- **Coverage target**: 80% line coverage on services and utilities. Routes and UI components exempt.
- **Integration tests use Ollama** — must run in CI without external API keys.

---

## 7. Documentation

### 7.1 Code comments

- **Comments explain WHY, not WHAT.** The code already shows what.
- **TODO format**: `// TODO(username, YYYY-MM-DD): description` — must include reference (issue/ticket) for non-trivial TODOs
- **No commented-out code in committed PRs.** Use git history.
- **JSDoc on all exported functions** — describe purpose, parameters, return value, errors thrown
- **Rust: `///` doc comments on all public items.** Run `cargo doc` periodically to verify

### 7.2 Module-level docs

- Every package has a `README.md` explaining: purpose, public API, usage example, dev setup
- Every service exports an interface; the interface file is the contract documentation

### 7.3 Architecture Decision Records (ADRs)

- Significant architectural decisions captured in `docs/adr/NNN-title.md`
- Format: Context, Decision, Consequences
- Update or supersede when decisions change — never delete

---

## 8. Git Workflow

### 8.1 Branching

- `master` (or `main`) — protected, always deployable, requires green CI
- Feature branches: `feat/<scope>/<short-desc>` (e.g., `feat/ai-pipeline/rag-index`)
- Bug fixes: `fix/<scope>/<short-desc>`
- Chores: `chore/<scope>/<short-desc>`

### 8.2 Commits

- **Conventional Commits format**: `<type>(<scope>): <description>`
- Types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `perf`, `build`, `ci`, `style`
- Scope optional but encouraged
- Description: imperative mood, lowercase, no period
- Examples:
  - `feat(ai-pipeline): add semantic chunking with tree-sitter`
  - `fix(provider): handle ollama timeout gracefully`
  - `docs(readme): update setup instructions`
- Body explains the **why** (not the what), wrapped at 72 chars
- Reference issues: `Closes #123` or `Refs #456`
- **One commit = one logical change.** Don't bundle unrelated changes.

### 8.3 Pull Requests

- PR title follows commit format
- PR description must include: what changed, why, how to test, screenshots (UI changes), breaking changes called out explicitly
- All PRs require: green CI, at least one approving review, no unresolved comments
- **Squash merge** to keep `master` history clean (configurable per repo)
- **No force-push to shared branches** (`master`, `develop`)
- **Delete branch after merge**

---

## 9. Security

- **Never commit secrets** to git — even temporarily. Use `.env` (gitignored) and `.env.example` (template).
- **Validate all user input** at the boundary (HTTP layer) using Zod schemas.
- **Encrypt sensitive data at rest** — user API keys via AES-256.
- **Use parameterized queries** — never string-concatenate SQL.
- **No `eval()`, `new Function()`, or dynamic code execution.**
- **Never execute uploaded code** — the IDE performs static analysis only.
- **Sanitize file paths** — prevent directory traversal attacks (`../../etc/passwd`).
- **Whitelist file extensions** for upload — never blacklist.
- **CSP headers** on web app, capability-based permissions on Tauri
- **Audit dependencies** — `pnpm audit` and `cargo audit` run in CI; high/critical vulns block merge
- **Secrets in CI** — use GitHub Actions secrets, never log them, never echo them
- **API keys redacted in logs** — implement masking utility

---

## 10. Performance

- **Profile before optimizing.** Don't guess.
- **Lazy-load** heavy dependencies (Tree-sitter grammars, Monaco languages)
- **Stream LLM responses** — never wait for full completion before sending to UI
- **Paginate** any list that can grow (files, chunks, artifacts)
- **Memoize** expensive computations (`useMemo`, `useCallback` only when measured benefit)
- **Avoid premature React re-renders** — co-locate state, split contexts, use Zustand selectors
- **Database queries** — N+1 is forbidden; use joins or batch loads
- **Don't block the main thread** — heavy work in workers (Web Workers, tokio tasks)

---

## 11. Dependencies

- **Prefer fewer, well-maintained dependencies** over many small ones
- **Justify new dependencies** in the PR description: why this, why not native, alternatives considered
- **Pin major versions** (caret ranges OK for minor: `^1.2.3`)
- **Update via Renovate / Dependabot** — review automated PRs, don't merge blindly
- **No abandoned packages** — last commit > 12 months = red flag
- **License check** — only MIT, Apache 2.0, BSD, ISC, MPL-2.0 allowed; GPL/AGPL/SSPL forbidden in production deps without legal review

---

## 12. AI / LLM-Specific Rules

### 12.1 Prompt management

- Prompts live in `src/prompts/` as `.ts` files exporting typed functions
- Each prompt versioned (e.g., `testPlanV2`) — never silently mutate live prompts
- Test prompts against Ollama (lowest-capability model) first; if it works there, it works on bigger models
- All prompts produce structured output via JSON Schema / function calling — no free-form parsing of natural language

### 12.2 Provider abstraction

- All LLM calls go through `LlmProvider` interface — no direct SDK calls in services
- Provider capabilities advertised via interface (e.g., `supportsToolUse`, `maxContextTokens`) — services check capability, not provider identity
- Streaming is first-class — every provider implements `stream()`

### 12.3 RAG / embeddings

- Embeddings generated lazily, cached by content hash
- Vector queries always include metadata filters (project_id, file_type) — never global search
- Top-K capped at 50; rerank if more candidates needed

### 12.4 Cost / safety

- **Token counting** before every LLM call — log input/output tokens for observability
- **Hard limits** on input size — reject requests > model context window with clear error
- **Redact secrets** from any code sent to LLM (regex scan for API key patterns)
- **No silent retries on LLM failures** — propagate errors with provider/model info

---

## 13. Anti-Patterns (Forbidden)

| Anti-pattern | Why forbidden | Alternative |
|--------------|---------------|-------------|
| Mutable global state | Untestable, race conditions | Dependency injection |
| `any` type in TS | Defeats type system | `unknown` + type guards |
| Throwing strings | No stack trace, can't catch by type | Throw `Error` subclasses |
| Hardcoded secrets | Security incident waiting | Env vars + secret manager |
| God objects / files >500 lines | Hard to understand, change | Split by responsibility |
| Deep inheritance hierarchies | Tight coupling | Composition |
| Boolean flag parameters | Unclear at call site | Separate functions or enum |
| Optional chaining as null check (`x?.y` to mean "x might be undefined") without verifying behavior | Can hide bugs | Explicit null check |
| Catching and ignoring errors (`try {} catch {}`) | Hides bugs | Handle or rethrow with context |
| `process.env.X` scattered through code | Untyped, hard to find | Centralized typed `config` module |
| Inline styles / inline event handlers in JSX (heavy) | Re-render churn | Extracted, memoized |
| Comments that describe what code does | Code rot, becomes lies | Self-documenting names; comments only for WHY |
| Premature abstraction | YAGNI; abstractions chose wrong axes | Inline first, extract on second use |

---

## 14. AI Coding Agent Instructions

> Read this section before generating code in this repository.

When generating code for this repo, you (the AI agent) must:

1. **Read this entire `rules.md` file** before producing code. Reference specific sections when explaining choices.
2. **Match the existing project structure** (Section 4). Don't invent new top-level directories without justification.
3. **Use TypeScript strict mode features** — no `any`, no non-null assertions, full type coverage.
4. **Write tests alongside implementation** (Section 6). A change without tests is incomplete.
5. **Follow Conventional Commits** when suggesting commit messages (Section 8).
6. **Cite the rule** when declining to do something. Example: "Per rules.md §13, I'm avoiding a god file — splitting this into three modules instead."
7. **Ask before introducing a new dependency.** Justify with the criteria in Section 11.
8. **Never invent APIs.** If unsure of a library's interface, look it up or ask. No hallucinated method names.
9. **Prefer composition over inheritance.** Prefer pure functions over classes when state isn't required.
10. **Fail loud** — wrap fallible operations in `try/catch` only when you can add value (context, recovery). Otherwise let errors propagate.
11. **Comments are for WHY only.** Don't add `// increment counter` comments.
12. **One commit per logical change.** Don't bundle unrelated edits. If you find a bug while doing something else, note it as a separate task.
13. **Generate PR descriptions** with: what, why, how to test, breaking changes, screenshots if UI.
14. **If the rule is unclear or seems wrong for the situation, surface it** — don't silently violate. Suggest a rules update via PR.

---

## 15. Code Review Checklist

Use this checklist on every PR (reviewer + author):

- [ ] CI green (lint, type check, tests, build)
- [ ] No `any`, no `// @ts-ignore`, no `unwrap()` (except documented)
- [ ] Tests added for new code; existing tests still pass
- [ ] No commented-out code
- [ ] No console.log / println! debug statements
- [ ] No secrets in diff
- [ ] Error handling explicit (no swallowed errors)
- [ ] Public API has docs (JSDoc / `///`)
- [ ] No new dependencies without justification
- [ ] Naming follows Section 3
- [ ] File structure follows Section 4
- [ ] Commit messages follow Conventional Commits
- [ ] PR description complete
- [ ] No breaking changes without explicit callout

---

## 16. Updating These Rules

- Rules evolve — propose changes via PR to this file
- New rules require team agreement (at least 2 approvals)
- Rules removal must include reasoning in commit message
- Major changes documented as ADR (`docs/adr/`)

---

**Last updated**: 2026-05-02
**Status**: Active. Enforced.
