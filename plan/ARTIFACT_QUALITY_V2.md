# Artifact Quality v2 — industry-grade generated artifacts

> Status: Planned | Owner: TBD | Created: 2026-06-06

Upgrade the four generated artifact schemas (Test Cases, Bug Report, Test Plan,
Defect Report) from basic to industry-grade, aligned with IEEE 829,
ISO/IEC/IEEE 29119-3, ISTQB, and TestRail conventions.

## 1. Goal

- Generated artifacts pass for professional QA deliverables — per-step expected
  results, steps-to-reproduce, severity/priority separation, plan scope and
  entry/exit criteria.
- Schemas mandate quality (negative + boundary cases, numbered repro steps) so
  the model cannot emit happy-path-only output that validates.

## 2. Non-goals

- No new artifact kinds.
- No UI redesign beyond rendering the new fields.
- `context_md_v1` unchanged.
- Sandbox runner contract unchanged — `files[]` payload on the test-cases
  artifact stays as-is.

## 3. Current state

| Piece | Where |
|---|---|
| Prompt modules | `apps/desktop/src-tauri/src/prompts/{test_cases,bug_report,defect_report,test_plan}_v1.rs` — tools `emit_test_cases`, `emit_bug_report`, `emit_defect_report`, `emit_test_plan` |
| Zod mirrors | `packages/shared/src/schemas/` — `TestCaseSchema`, `DefectReportSchema`, `TestPlanSchema`. **Bug report has no dedicated Zod schema** (free-form JSON in `structuredData`) |
| Rendering | `apps/desktop/src/components/ai-panel/artifact-detail-drawer.tsx` (MarkdownView; `SandboxRunPanel` for test-cases) |
| Prompt snapshots | `apps/desktop/src-tauri/src/prompts/snapshots.rs` + `prompts/snapshots/*.snap` (insta) |

## 4. Gap analysis

| Artifact | Has today | Missing vs standard |
|---|---|---|
| Test Cases | id, title, priority p0–p3, preconditions[], steps[] (plain strings), single expectedResult, traceability[], files[] | per-step expected results (TestRail separated-steps), test data, case type (negative/boundary/security), postconditions, design-technique mandates (BVA, equivalence partitioning) |
| Bug Report | id, description, expected/actual, environment, evidence, severity (3-level) | **steps-to-reproduce**, severity↔priority split, reproducibility, workaround, component, 5-level severity |
| Test Plan | summary, objectives, strategy, environments, risks | scope in/out, entry/exit criteria, suspension criteria, test levels & types, deliverables (IEEE 829 backbone) |
| Defect Report | category, confidence, impact, location, severity | CWE-aligned classification, fix suggestion, evidence parity with bug report |

## 5. Phased build — 3 phases, each its own branch, green CI, squash-merge

### Phase 1 — Core evidence artifacts (`feat/artifact-evidence-v2`)

Test cases v2 (`prompts/test_cases_v2.rs`, VERSION `test_cases_v2`, v1 kept):
- [ ] `steps` becomes `{ action, expectedResult }[]` (separated-steps pattern).
- [ ] Add `testData?` (string), `type` enum `positive|negative|boundary|error|security`, `postconditions[]`.
- [ ] Prompt mandates ≥1 negative and ≥1 boundary case per covered feature.
- [ ] `files[]` payload byte-identical contract — sandbox (`RunInput`, `sandbox_service`) untouched.
- [ ] Update `TestCaseSchema` Zod + round-trip contract test, same PR.

Bug report v2 (`prompts/bug_report_v2.rs`, VERSION `bug_report_v2`):
- [ ] Add `stepsToReproduce[]` (minItems 1, numbered actions).
- [ ] Split `severity` (`blocker|critical|major|minor|trivial`) from `priority` (`p0|p1|p2|p3`).
- [ ] Add `reproducibility` enum `always|intermittent|once`, `workaround?`, `component?`.
- [ ] Create dedicated `packages/shared/src/schemas/bug-report.schema.ts` + round-trip contract test (closes the free-form-JSON gap).

Shared:
- [ ] `generation_service` routes both artifacts to v2 modules.
- [ ] `artifact-detail-drawer.tsx` renders step tables + new fields.
- [ ] Insta snapshots added for both v2 modules.

### Phase 2 — Planning + analysis artifacts (`feat/artifact-planning-v2`)

Test plan v2 (`prompts/test_plan_v2.rs`, VERSION `test_plan_v2`) — 29119-lite:
- [ ] Add `scope { inScope[], outOfScope[] }`.
- [ ] Add `entryCriteria[]`, `exitCriteria[]`, `suspensionCriteria[]`.
- [ ] Add `testLevels[]` (unit/integration/e2e), `testTypes[]` (functional/perf/security/...), `deliverables[]`.
- [ ] Update `TestPlanSchema` Zod + contract test.

Defect report v2 (`prompts/defect_report_v2.rs`, VERSION `defect_report_v2`):
- [ ] `category` enum aligned to CWE top classes (input validation, auth, resource mgmt, logic, error handling, concurrency).
- [ ] Add `fixSuggestion` per finding.
- [ ] Evidence fields at parity with bug report (`evidence_snippet`, `file_hint`, line range).
- [ ] Update `DefectReportSchema` Zod + contract test.

Shared:
- [ ] Routing, drawer rendering, insta snapshots — same mechanics as Phase 1.

### Phase 3 — Prompt quality + verification (`feat/artifact-prompt-quality`)

- [ ] Few-shot industry-grade exemplar in each v2 prompt (one full example artifact).
- [ ] Technique mandates in prompts: boundary-value analysis + equivalence partitioning for test cases; what/where/when description discipline for bugs.
- [ ] Regenerate all insta snapshots; review diffs.
- [ ] Golden integration tests vs live Ollama (`test:integration`) for all four v2 artifacts.
- [ ] Token-budget re-check: richer schemas → larger outputs; verify against the budget guard in `generation_service`.
- [ ] End-to-end: generate all 4 artifacts on a sample repo; sandbox run still executes off the test-cases artifact.

## 6. Cross-cutting rules (rules.md)

- Rust serde drives Zod (§12.3.1) — schema change + Zod mirror + covering tests in the same PR.
- Prompts versioned: new `*_v2.rs` module with `VERSION` const; v1 modules stay (replay/back-compat).
- Discriminator strings match exactly between serde rename and `z.literal`.
- No `any` / non-null assertions / `as` casts in TS; schema-validate all LLM output via `jsonschema`.

## 7. Acceptance criteria

1. Each generated artifact validates against its v2 JSON-Schema on the default
   (Ollama) path and on cloud providers.
2. New fields render in the artifact drawer (step tables for test cases,
   repro steps for bugs, scope/criteria sections for plans).
3. Insta snapshot + Zod contract tests green; CI green end to end.
4. Sandbox run path unaffected — opt-in run off a v2 test-cases artifact
   passes/fails identically to v1.
5. Spot check: a generated bug report contains numbered repro steps, split
   severity/priority, and reproducibility — without manual editing.

## 8. Sources

- ISO/IEC/IEEE 29119-3:2021 — test documentation: <https://www.iso.org/standard/79429.html>
- IEEE 829 test plan outline: <https://jmpovedar.wordpress.com/wp-content/uploads/2014/03/ieee-829.pdf>
- TestRail test case templates: <https://support.testrail.com/hc/en-us/articles/14927678348052-Test-case-templates>
- TestRail effective test cases: <https://www.testrail.com/blog/effective-test-cases-templates/>
- ISTQB defect report definition: <https://glossary.istqb.org/en_US/term/defect-report/1>
- QA Wolf — what makes a great bug report: <https://www.qawolf.com/blog/what-makes-a-great-bug-report>
