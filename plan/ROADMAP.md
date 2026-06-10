# Tessera Roadmap

Forward-looking view: known limitations of the shipped **v0.1** and the
features planned to address them. v0.1 itself is feature-complete (5 artifact
types, 5 LLM providers, RAG pipeline, streaming, cross-platform releases) — see
the [README](../README.md).

---

## Known limitations & planned solutions

| Limitation | Impact | Planned solution |
|---|---|---|
| **Single-user only** — no team collaboration, sharing, or multi-user workspace | Limits enterprise adoption | Workspace sync via CRDTs (Yjs/Automerge) + optional cloud relay, keeping the local-first core |
| ~~**No test execution**~~ — **shipped for JS/TS:** opt-in Docker sandbox runs generated cases and reports pass/fail + coverage | Closed the generate→run→measure loop | Python (`docker_py`) + cloud runners next, behind the same `TestRunner` trait |
| ~~**Embedding provider lock-in**~~ — **shipped:** selectable embedding provider — local Ollama (default) or OpenAI / Gemini / Hugging Face cloud | RAG no longer gated on a local GPU | Done — see [`EMBEDDING_PROVIDER_SELECT.md`](./EMBEDDING_PROVIDER_SELECT.md). Possible future additions: Voyage AI / Cohere |
| **Minimal E2E coverage** — one Playwright spec, no error-path tests | UI-flow regressions go undetected | Expand to 10–15 specs: generation flow, provider switching, error states, export |
| ~~**No export integrations**~~ — **shipped:** Excel/CSV/TSV + copy-as-TSV, Markdown + JSON export, and Jira Cloud push v1 (idempotent, per-artifact) | Artifacts flow to spreadsheets and Jira | Remaining: Jira v2 — epic/child bulk push, sandbox-run comments, status refresh, severity-map editor — Phase 3 in [`JIRA_INTEGRATION.md`](./JIRA_INTEGRATION.md). Linear / GitHub Issues adapters behind the same `IssueTracker` trait |
| **No observability** — no coverage reports, perf metrics, or usage analytics | Hard to track quality over time | LCOV in CI, opt-in telemetry (PostHog/Plausible), bundle-size tracking |
| **Static prompts** — v1 prompts are hardcoded | Power users can't tune generation | User-editable prompt templates with variable substitution + prompt A/B testing |
| **Basic artifact schemas** — *mostly closed:* v2 IEEE 829 / ISO 29119-3 schemas shipped for all four artifacts (Phases 1–2 of [`ARTIFACT_QUALITY_V2.md`](./ARTIFACT_QUALITY_V2.md)) | Artifacts now carry repro steps, severity/priority split, scope + entry/exit criteria | Remaining: Phase 3 — few-shot exemplars in prompts, technique mandates (BVA / equivalence partitioning), golden integration tests vs live Ollama, token-budget re-check |

---

## Planned standout features

### 1. Live test runner with coverage overlay — **shipped (JS/TS)**
Generate test cases → execute them in a sandboxed Docker container → show pass/fail +
line coverage directly on the Monaco editor.
**Edge:** closes the full generate → run → measure loop in one tool — no other AI
testing tool does.
**Status:** JS/TS vertical slice shipped (opt-in, off by default) — see
[`SANDBOX_TEST_RUNNER.md`](./SANDBOX_TEST_RUNNER.md) and
[ADR-0004](../apps/desktop/src-tauri/docs/adr/0004-sandbox-test-runner.md). Python
(`docker_py`) + cloud runners reuse the same `TestRunner` trait next.

### 2. Mutation testing integration
After generating tests, mutate the source (flip operators, drop conditions) and check
whether the tests catch the mutations, reporting a **mutation score** alongside coverage.
**Edge:** proves test quality objectively, not just a coverage percentage.

### 3. Diff-aware incremental generation
Watch git diffs (pre-commit hook or file watcher); when code changes, regenerate only
the affected test cases and flag a "stale tests" badge on cases that reference modified
functions. **Edge:** keeps test artifacts in sync with live code — the biggest manual-
testing pain point.

### 4. Multi-model consensus panel
Run the same prompt against 2–3 models simultaneously (e.g. Ollama + OpenAI + Anthropic),
show artifacts side by side, let the user cherry-pick the best sections, and highlight
where models disagree (likely edge cases worth extra attention). **Edge:** no competitor
offers multi-model consensus for test generation.

### 5. Test impact graph
Build a call graph from the AST (tree-sitter is already in place), visualize which test
cases cover which functions, and highlight the tests that need re-review when a function
changes — rendered as an interactive force-directed graph. **Edge:** turns Tessera from a
"test generator" into a "test intelligence platform."
