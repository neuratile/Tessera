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
| **No test execution** — generates test artifacts but can't run them | Users copy-paste tests elsewhere | Sandboxed runner (Docker/Wasm) that executes generated cases and reports pass/fail |
| **Embedding provider lock-in** — Ollama embeddings only, no cloud fallback | RAG quality bottleneck without a local GPU | OpenAI / Voyage AI / Cohere embedding providers behind the existing `EmbeddingProvider` trait |
| **Minimal E2E coverage** — one Playwright spec, no error-path tests | UI-flow regressions go undetected | Expand to 10–15 specs: generation flow, provider switching, error states, export |
| **No export integrations** — artifacts live only in local SQLite | Can't push to JIRA / Linear / GitHub Issues | Per-platform export adapters + clipboard-friendly Markdown |
| **No observability** — no coverage reports, perf metrics, or usage analytics | Hard to track quality over time | LCOV in CI, opt-in telemetry (PostHog/Plausible), bundle-size tracking |
| **Static prompts** — v1 prompts are hardcoded | Power users can't tune generation | User-editable prompt templates with variable substitution + prompt A/B testing |

---

## Planned standout features

### 1. Live test runner with coverage overlay
Generate test cases → execute them in a sandboxed environment (Docker container or
Wasm per language) → show pass/fail + line coverage directly on the Monaco editor.
**Edge:** closes the full generate → run → measure loop in one tool — no other AI
testing tool does.

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
