# Tessera v2 — Vision & Prioritized Feature List

> Status: **draft** — research + prioritization done (2026-06-10); individual
> feature design docs land under [`v2-feature-docs/`](./v2-feature-docs/) as they
> are specced (first: flaky-test detection, P2 #7) · Owner: core

## 1. Theme

**From test generator to autonomous test quality platform — still 100% local.**

v1 closed the generate → run → measure loop (the only AI testing tool that
does, locally). v2 weaponizes that loop: tests that prove and repair
themselves, an objective quality score beyond coverage, and distribution
channels (CLI / CI / MCP) that put Tessera inside open-source pipelines and
agent workflows instead of only behind a desktop GUI.

Tagline candidate: *"CI-grade AI tests, zero code leaves your machine."*

## 2. Market context (research, June 2026)

- **Qodo 2.0** (Feb 2026) moved to a multi-agent review architecture with
  parallel bug / security / coverage-gap agents, plus a CLI for terminal and
  CI workflows. Best-in-class F1 on review benchmarks.
- **Diffblue Cover** sells autonomous CI-integrated unit-test generation for
  Java at enterprise scale (claims 20× productivity vs. coding assistants).
- **Keploy** (OSS) records real API traffic and converts it into tests + mocks.
- **Meta's ACH** validated LLM-driven mutation testing at scale: 73% of
  generated mutation-killing tests accepted by engineers. A suite can have
  100% line coverage and a 4% mutation score — coverage alone is a weak metric.
- **Trends**: self-healing tests (60–80% maintenance reduction claims),
  AI-generated tests as standard CI gates, MCP everywhere (~97M monthly SDK
  downloads), local-first privacy now a *buying criterion*, not a nice-to-have.
- **Pain points** (Stack Overflow 2026): 66% of devs cite "AI solutions almost
  right but not quite"; test *maintenance* (stale/flaky), not test *writing*,
  is the #1 grind.

Nobody combines local-first + multi-provider + sandboxed execution. That is
the moat v2 builds on.

## 3. Prioritized features

### P0 — close the autonomous quality loop

1. **Agentic self-healing loop.** Generate → run in sandbox → on failure, feed
   the error output + source context back to the LLM → regenerate the failing
   case → rerun, with bounded retries. All plumbing exists
   (`generation_service` + `sandbox_service`); this composes them. Directly
   attacks "almost right but not quite" — tests prove themselves before the
   user sees them.
2. **Mutation testing + mutation score.** Mutate the source AST (tree-sitter
   already in place: operators flipped, conditions dropped), rerun the suite
   in the same hardened Docker harness, report killed/survived mutants next to
   line coverage. Promoted from ROADMAP standout #2 on the strength of Meta's
   ACH results.
3. **Headless CLI + GitHub Action.** `tessera generate --diff`,
   `tessera run`, machine-readable output, CI-friendly exit codes; a published
   Action so any OSS repo gets generated tests + coverage + mutation score as
   a PR check. This is the primary open-source adoption channel — the GUI
   alone never reaches maintainers at scale.

### P1 — distribution + maintenance

4. **MCP server mode.** Expose generate / run / score as MCP tools so Claude
   Code, Cursor, and other agents can drive Tessera. Cheap once the CLI
   exists; makes Tessera infrastructure for other agents rather than a
   competitor to them.
5. **Diff-aware stale-test detection** (ROADMAP standout #3). Watch git diffs,
   flag test cases referencing changed functions, one-click incremental
   regeneration. Pairs with the self-healing loop; hits the #1 reported pain.
6. **Java + Go sandbox runners.** Go AST support already shipped (#75); the
   `TestRunner` trait, shared Docker harness, and open-TEXT `runner` column
   absorb new languages without migration. Java competes directly with
   Diffblue's enterprise-only niche, free and local.

### P2 — differentiation + polish

7. **Flaky-test detection.** Run the suite N times in the sandbox, flag
   non-deterministic cases. Cheap given the harness. **First v2 feature
   specced *and shipped*** (first slice) — design doc:
   [`v2-feature-docs/FLAKY_TEST_DETECTION.md`](./v2-feature-docs/FLAKY_TEST_DETECTION.md).
   N-run loop (default 5, adjustable 2–20) + per-test
   stable-pass / stable-fail / flaky verdict, reusing the v1 sandbox harness
   with no DB migration. Hardening (persisted history, CLI/Action gate,
   auto-quarantine) deferred to follow-up docs.
8. **Multi-model consensus panel** (ROADMAP standout #4). Same prompt against
   2–3 providers, side-by-side artifacts, disagreement highlighting.
9. **Test impact graph** (ROADMAP standout #5). Call-graph visualization of
   which cases cover which functions; feeds off the diff-aware feature.
10. **User-editable prompt templates + community template sharing.** Versioned
    prompt infra exists; sharing creates a community flywheel.

### P3 — later / reassess

11. CRDT workspace sync (heavy; niche until team adoption exists).
12. Keploy-style API traffic capture (large scope, different product shape).
13. Quality-over-time dashboards (needs the telemetry foundation first).

## 4. Phasing

| Phase | Scope | Items |
|---|---|---|
| **A — Quality loop** | Self-healing repair loop + mutation score | P0 #1, #2 |
| **B — Distribution** | CLI + GitHub Action + MCP server | P0 #3, P1 #4 |
| **C — Maintenance + reach** | Diff-aware stale detection + Java/Go runners | P1 #5, #6 |

Phase B carries the open-source-community goal: an Action badge on READMEs is
the distribution; the desktop app is the workbench.

## 5. Sources

- Qodo 2.0 multi-agent review: <https://www.qodo.ai/> ·
  <https://dev.to/rahulxsingh/qodo-ai-review-2026-is-it-the-best-ai-testing-tool-31hj>
- Diffblue agent orchestration / 20× claim:
  <https://www.diffblue.com/resources/orchestrating-coding-agents-to-automate-regression-unit-test-generation-at-scale/> ·
  <https://www.businesswire.com/news/home/20251104720918/en/>
- Keploy AI testing tools overview: <https://keploy.io/blog/community/ai-testing-tools>
- Meta — LLMs are the key to mutation testing:
  <https://engineering.fb.com/2025/09/30/security/llms-are-the-key-to-mutation-testing-and-better-compliance/> ·
  InfoQ summary: <https://www.infoq.com/news/2026/01/meta-llm-mutation-testing/>
- 2026 testing trends: <https://www.parasoft.com/blog/annual-software-testing-trends/> ·
  <https://testomat.io/blog/software-testing-trends/> ·
  <https://www.buildmvpfast.com/blog/ai-testing-automation-self-healing-qa-maintenance-2026>
- Stack Overflow survey takeaways:
  <https://adtmag.com/blogs/watersworks/2026/01/stack-overflow-survey.aspx>
- Local-first AI tools 2026: <https://nimbalyst.com/blog/best-local-first-ai-coding-tools-2026/>
- Claude Code + MCP test generation:
  <https://testcollab.com/blog/automated-test-case-generation-claude-code-mcp>
