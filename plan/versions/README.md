# plan/versions — design docs by release

Design docs for multi-day features, grouped by the release they shipped in (or
are targeted at). This gives contributors a per-version record of what was
planned, what shipped, and the reasoning behind it.

```
plan/
  ROADMAP.md          forward-looking: limitations + planned features (never versioned)
  versions/
    v1/               docs for features shipped in the v1 (v0.1.x) release line
    v2/               docs for features targeted at the v2 release line
```

Conventions:

- A doc lives in the folder of the version it **ships** in. Docs for in-flight
  work sit in the target version's folder with a `Status:` line at the top.
- `ROADMAP.md` stays at `plan/` root — it spans versions.
- When a feature slips a release, move its doc to the new version folder and
  fix inbound links (code comments reference these paths too — grep before moving).

## v1 (shipped)

| Doc | Feature |
|---|---|
| [`SANDBOX_TEST_RUNNER.md`](./v1/SANDBOX_TEST_RUNNER.md) | Opt-in Docker sandbox test runner (JS/TS) — pass/fail + line coverage |
| [`SANDBOX_PYTHON_RUNNER.md`](./v1/SANDBOX_PYTHON_RUNNER.md) | Python sandbox runner (`docker_py`) + shared Docker hardening harness |
| [`EMBEDDING_PROVIDER_SELECT.md`](./v1/EMBEDDING_PROVIDER_SELECT.md) | Selectable embedding provider (Ollama local / OpenAI / Gemini / Hugging Face) |
| [`ARTIFACT_QUALITY_V2.md`](./v1/ARTIFACT_QUALITY_V2.md) | IEEE 829 / ISO 29119-3 v2 artifact schemas (Phases 1–2 shipped; Phase 3 open) |
| [`ARTIFACT_EXPORT.md`](./v1/ARTIFACT_EXPORT.md) | Excel/CSV/TSV, Markdown/JSON export + copy actions |
| [`JIRA_INTEGRATION.md`](./v1/JIRA_INTEGRATION.md) | Jira Cloud push v1 (idempotent, per-artifact; Phase 3 open) |
| [`TEST_CASE_TABLE.md`](./v1/TEST_CASE_TABLE.md) | Fixed 9-column Test Case table + execution-outcome sidecar |
| [`CONNECTION_SELECT.md`](./v1/CONNECTION_SELECT.md) | Explicit active-LLM-connection selection (singleton config) |
| [`CI_JOB_CONSOLIDATION.md`](./v1/CI_JOB_CONSOLIDATION.md) | CI pipeline consolidation |

## v2 (planned)

| Doc | Feature |
|---|---|
| [`V2_VISION.md`](./v2/V2_VISION.md) | v2 theme, research findings, and prioritized feature list |
| [`v2-feature-docs/FLAKY_TEST_DETECTION.md`](./v2/v2-feature-docs/FLAKY_TEST_DETECTION.md) | Flaky-test detection — run the suite N times, flag non-deterministic cases |
| [`v2-feature-docs/SELF_HEALING_LOOP.md`](./v2/v2-feature-docs/SELF_HEALING_LOOP.md) | Agentic self-healing loop — run → diagnose → regenerate → rerun |
| [`v2-feature-docs/MUTATION_TESTING.md`](./v2/v2-feature-docs/MUTATION_TESTING.md) | Mutation testing — mutation score (Stage 1) + auto-improve survivors (Stage 2) |
| [`v2-feature-docs/V2_HARDENING.md`](./v2/v2-feature-docs/V2_HARDENING.md) | Quality-loop hardening before distribution — heal history, equivalence mutants, focused regen, result contract |
