# v2 quality-loop hardening (pre-distribution)

> Status: **draft** (v2, Phase A→B bridge) — design only, not yet built · Owner: core
> Depends on: `healing_service::heal`, `mutation_service::{score, improve}`,
> `sandbox_service::{run, run_flaky}` and the mutant engine
> (`providers/runners/mutation.rs`) — all shipped, all reused. No new LLM prompt,
> no new output schema. One additive migration (`0010`).
>
> This doc collects the **deferred-but-load-bearing** items from the three shipped
> v2 quality features and sequences them *before* the headless CLI + GitHub Action
> (V2_VISION P0 #3). The CLI turns these loops into an unattended public PR check;
> the gaps below are the ones that would otherwise get baked into that surface
> permanently. Everything here hardens existing behaviour — no new feature axis.

## 0. Where this sits in v2

v2's theme (V2_VISION §1): **"from test generator to autonomous test-quality
platform — still 100% local."** Phase A shipped the three quality axes:

- **Self-healing** (P0 #1, shipped) — tests repair themselves on failure.
- **Mutation score + improve** (P0 #2, shipped) — proves the suite catches bugs.
- **Flaky-test detection** (P2 #7, shipped) — can a test be trusted twice.

Phase B (V2_VISION §4) is **distribution**: headless CLI + GitHub Action + MCP.
The moment these loops run in CI, three things stop being cosmetic:

1. A CI run with **no audit trail** is hard to trust — and self-healing is the one
   axis with no persisted history (flaky `0008`, mutation `0009` both have one).
2. A mutation-score **gate that fails on un-killable mutants** rejects good PRs —
   equivalence mutants depress the score through no fault of the suite.
3. **Whole-artifact regeneration** is wasteful and risks regressing green tests —
   tolerable behind a GUI with a human reviewing, costly when run unattended at
   scale, and it has no stable machine-readable contract for a CI to parse.

This doc fixes those three before the CLI consumes them, so the CLI is built on
top of correct, auditable, machine-parseable loops rather than refactored later.

## 0.1 End state — what changes when this ships

Nothing new appears as a top-level feature; the existing actions get *trustworthy*:

- **Self-heal** gains a **"Heal history"** trend in the sandbox panel, exactly like
  the Flaky and Mutation trends — every completed heal is one persisted row, so a
  QA engineer (or a CI log) can see "this suite has needed healing 4 of the last 6
  generations" and which tests keep regressing.
- **Mutation score** now reports survivors split into **"survived"** (a real gap the
  suite missed) and **"equivalent — excluded"** (a mutant that cannot change
  observable behaviour, so no test could ever kill it). The headline score is
  computed on killable mutants only, so it stops being unfairly depressed — and
  Stage 2 "Improve coverage" no longer wastes LLM tokens chasing un-killable
  survivors.
- **Self-heal** and **Improve coverage** regenerate **only the failing / surviving
  test cases**, not the whole artifact — cheaper, faster, and a passing test can no
  longer be collateral-damaged by a regeneration aimed at a different case.
- Every orchestrator (`heal`, `score`, `improve`, `run_flaky`) returns a **versioned,
  machine-readable result envelope** with stable field names and a deterministic
  **exit-code mapping** — the contract the CLI/Action will serialize verbatim.

Still 100% local. No new opt-in, no new provider call, no new artifact type.

## 1. Problem

The three shipped loops each parked a deferred item that is invisible behind the
desktop GUI but becomes load-bearing the instant the loop runs headless in CI:

| Loop | Deferred item (doc) | Why it bites in CI |
|---|---|---|
| Self-heal | persisted heal history / trend — needs a migration (`SELF_HEALING_LOOP.md` §3, §7) | A CI heal has no audit trail; can't trend regressions across runs |
| Mutation | equivalence-mutant detection (`MUTATION_TESTING.md` §3 "Out") | Un-killable survivors depress the score → a mutation gate fails good PRs and `improve` burns tokens chasing them |
| Self-heal + Improve | focused per-case regeneration (`SELF_HEALING_LOOP.md` §7, `MUTATION_TESTING.md` §3) | Whole-artifact regen is N× the tokens and can regress already-green tests, unattended |
| All four | output is event-streamed for the GUI (`heal://event`, `mutation://event`, …) | No stable JSON contract / exit codes for a machine to consume |

None of these need a new feature; all are tightening of code that already exists.

## 2. Approach

Three phases, each an independently shippable PR, ordered by how directly the CLI
depends on the result. Each reuses the existing orchestrators verbatim except for
the one seam it hardens.

1. **Heal history (parity).** Add migration `0010_heal_checks.sql` (additive,
   mirrors `0008`/`0009` exactly), a `heal_check_repo`, persistence inside
   `healing_service::heal` (best-effort, never discards the in-memory result —
   same contract as flaky/mutation history), `list_heal_checks` / `get_heal_check`
   commands, the Zod mirror + IPC wrapper, and a collapsible **"Heal history"**
   trend in the sandbox panel.

2. **Equivalence-mutant classification (correctness).** Extend the pure engine in
   `providers/runners/mutation.rs` to flag mutants whose replacement cannot change
   observable behaviour (the cheap, provable cases — e.g. dead-code / no-op edits
   on uncovered branches, idempotent operator swaps), add an `Equivalent` variant
   to `MutantStatus`, exclude it from the score denominator, and surface it in the
   survivor list as a distinct, non-actionable row. `improve` skips equivalent
   mutants when synthesizing survivor feedback.

3. **Focused regen + machine-readable contract (CLI-prep).** Change the
   feedback-synthesis step in `healing_service` and `mutation_service::improve` to
   regenerate only the failing / surviving cases (scoped `reviewer_feedback` +
   case selection) while keeping the `parent_id` version chain, and define a
   single versioned `QualityResult` envelope (+ exit-code mapping) that every
   orchestrator returns — the exact shape the CLI/Action will print.

## 3. Scope

In: the three phases above — one migration, one engine extension, one regen-scope
change + one result-contract definition, each with its command/IPC/UI surface and
tests. Reuses `generate` / `run` / `run_flaky` and the Docker harness unchanged.

Out (still deferred to the CLI doc and beyond): the CLI binary and GitHub Action
themselves (V2_VISION P0 #3 — this doc only prepares their inputs); higher-order
mutants (>1 edit); auto-quarantine of flaky tests; cross-run coverage aggregation;
Python/Go mutation operators; full equivalent-mutant proving (only the cheap,
provable subset is in scope — see §5.2 risk).

## 4. Design decisions

- **Hardening, not features.** Every item is a deferred line from an already-shipped
  doc. No new prompt, no new LLM-output schema, no new artifact type, no new opt-in
  gate — the Docker `sandboxOptIn` gate and existing schemas are reused.
- **History mirrors flaky/mutation move-for-move.** `0010` is additive only,
  every FK indexed, `run_id` FK `ON DELETE SET NULL` so history outlives a purged
  run (rules §2.3) — identical to `0008`/`0009`. Persistence is best-effort: a
  write failure logs and is swallowed, never discarding the in-memory heal result.
- **Equivalence detection is conservative.** Full equivalent-mutant detection is
  undecidable; we only flag the cheap *provable* cases and leave anything uncertain
  classified as `survived` (a false "real gap" is safe; a false "equivalent" hides
  a genuine test gap, so we never guess in that direction). The count of excluded
  equivalents is shown, never silently dropped (rules: a bounded exclusion must say
  so — same discipline as the mutant cap's `dropped_count`).
- **Focused regen keeps the version chain.** Per-case regeneration still produces a
  new `parent_id`-chained artifact version; only the `reviewer_feedback` scope and
  the set of cases sent for regen change. Best-attempt selection logic is unchanged.
- **One result envelope, defined once.** The `QualityResult` contract lives in
  `packages/shared` (Zod, Rust serde source of truth per §12.3.1) so the GUI, the
  future CLI, and the Action all serialize the identical shape — defined here, not
  re-invented inside the CLI PR.

## 5. Phase detail

### 5.1 Phase A — Heal history (parity)

- **Migration `0010_heal_checks.sql`** (additive; mirrors `0008`/`0009`):
  - `heal_checks` — one row per completed heal: `id`, `artifact_id`, `project_id`,
    `baseline_run_id` (nullable, `ON DELETE SET NULL`), `attempts`, `healed_count`,
    `still_failing_count`, `final_passing`, `final_total`, `landed_version_id`,
    `created_at`, `updated_at`. FKs to `artifacts` / `projects` cascade; indexes on
    `artifact_id`, `project_id`, `created_at`.
  - `heal_check_tests` — one row per test in the final attempt: `id`, `check_id`
    (FK `ON DELETE CASCADE`, indexed), `test_id`, `status`
    (`healed | still_failing | passed`, mirroring the heal verdict), `healed_at_attempt`
    (nullable), `last_failure_message` (nullable), timestamps.
- **`repositories/heal_check_repo.rs`** — batch insert (no N+1), `list_by_artifact`
  (paginated), `get_by_id`. Mirrors `mutation_check_repo`.
- **`healing_service::heal`** — after the loop settles, persist via `heal_check_repo`
  best-effort (a write error logs + is swallowed; the returned `HealResult` is
  unchanged). Add `list_heal_history` / `get_heal_check` read methods.
- **Commands** `list_heal_checks` / `get_heal_check` (`commands/healing.rs`),
  `Result<T, String>` boundary mapping.
- **Shared + FE** — `HealCheck*` Zod schemas (`packages/shared`) with round-trip
  contract test; typed IPC wrappers; store slice; collapsible **"Heal history"**
  trend in the sandbox panel, mirroring the flaky/mutation trend component.
- **Tests** — repo round-trip; `heal` persists-on-success and survives a forced
  repo error (scripted) without losing the result; Zod round-trip; status-string ↔
  serde-enum parity.

### 5.2 Phase B — Equivalence-mutant classification (correctness)

- **`providers/runners/mutation.rs`** — add `MutantStatus::Equivalent`; a pure
  `classify_equivalent(mutant, coverage)` that returns true only for the cheap
  provable cases (e.g. an edit on a line the baseline coverage proves is
  unexecuted, or an operator swap that is provably idempotent for the operand
  types in reach). Conservative by construction: unknown → `survived`.
- **`mutation_service::score`** — denominator becomes `killed + survived` (exclude
  `equivalent`); carry an `equivalent_count` alongside `dropped_count`. Survivor
  list renders equivalents as a distinct **non-actionable** row ("equivalent —
  excluded").
- **`mutation_service::improve`** — skip `equivalent` mutants when synthesizing
  survivor `reviewer_feedback` (no tokens spent on un-killable mutants).
- **Persistence** — `mutation_check_mutants.status` is already open TEXT; add the
  `equivalent` literal to the Zod mirror + `MutantStatus` serde enum (same PR,
  per §12.3.1). Add `equivalent` count to `mutation_checks` (additive column via a
  small `0011` migration, or reuse `errored` semantics — decide at build, prefer
  additive column for a clean trend).
- **Tests** — engine unit tests over a fixture matrix (each provable-equivalent
  shape → `Equivalent`; each uncertain shape → `survived`); score excludes
  equivalents from the denominator; `improve` ignores them.

### 5.3 Phase C — Focused regen + machine-readable contract (CLI-prep)

- **Scoped regeneration** — in `healing_service` (failures) and
  `mutation_service::improve` (survivors), synthesize `reviewer_feedback` for only
  the affected cases and pass a case-selection hint to `generate`, keeping
  `parent_id` chaining and best-attempt selection unchanged. Net effect: fewer
  tokens, no collateral regression of green cases.
- **`QualityResult` envelope** — define once in `packages/shared` (serde source of
  truth): `{ kind: 'heal' | 'mutation_score' | 'improve' | 'flaky', version,
  summary, per_item[], landed_version_id?, exit_code }` with a deterministic
  exit-code map (e.g. `0` clean, `1` unresolved failures/survivors over threshold,
  `2` infra/Docker error). Orchestrators return it; the GUI keeps using its richer
  event stream, the future CLI serializes this verbatim.
- **Tests** — Zod round-trip for `QualityResult`; per-orchestrator snapshot of the
  envelope for representative outcomes; focused-regen test proving a green case is
  untouched when a sibling case is regenerated.

## 6. Risks

- **Equivalence false-positives are the dangerous direction** — flagging a killable
  mutant as equivalent hides a real test gap. Mitigation: conservative classifier
  (§4), fixture matrix gating, and the visible `equivalent_count` so a reviewer can
  audit exclusions. When in doubt, classify `survived`.
- **Focused regen could under-feed the LLM** if a failure's true cause spans cases.
  Mitigation: keep whole-artifact regen as a fallback when scoped regen makes no
  progress (the existing no-progress stop condition already exists in `heal`).
- **Contract churn** — defining `QualityResult` before the CLI exists risks a
  reshape later. Mitigation: `version` field from day one; the GUI does not depend
  on it (keeps its event stream), so only the not-yet-built CLI consumes it.

## 7. Future items (deferred past this doc)

CLI binary + GitHub Action (V2_VISION P0 #3 — this doc only readies their inputs);
higher-order mutants; auto-quarantine of flaky tests; cross-run coverage
aggregation; Python/Go mutation operators; full (beyond-cheap) equivalent-mutant
proving; MCP server mode (P1 #4).
