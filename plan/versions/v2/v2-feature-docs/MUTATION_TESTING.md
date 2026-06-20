# Mutation testing + mutation score

> Status: **draft** (v2, P0 #2) — design only, not yet built · Owner: core
> Depends on: `sandbox_service::run` (execution entry point, v1, reused
> verbatim), `ast_service` (tree-sitter parsing, v1) and
> `generation_service::generate` (LLM entry point, v1 — Stage 2 only).
>
> Two stages, two user actions, two PRs. **Stage 1 (score)** mutates the
> source, reruns the existing suite per mutant in the existing Docker harness,
> and reports a kill/survive **mutation score** next to line coverage. **Stage 2
> (improve)** feeds survivors back to the LLM to auto-generate tests that kill
> them, then re-scores to prove it. Persisted history ships from the first PR.
> Language scope: **JS/TS only** in v1 (Python/Go reuse the same engine later).

## 0. Where this sits in v2

v2's theme (V2_VISION §1): **"from test generator to autonomous test-quality
platform — still 100% local."** The three quality axes v2 adds:

- **Self-healing** (P0 #1, *shipped*) — tests repair themselves on failure.
- **Mutation score** (P0 #2, *this doc*) — does the suite actually catch bugs.
- **Flaky-test detection** (P2 #7, *shipped*) — can a test be trusted twice.

Mutation testing is the **proof** axis and the second half of Phase A. It
directly attacks the weakest metric in the loop today: line coverage. A suite
can hit 100% line coverage and still catch ~4% of real defects (V2_VISION §2,
Meta ACH). Mutation score measures the thing coverage cannot — *would the suite
fail if the code were wrong?* — and Stage 2 closes the gap by writing the
missing tests automatically.

## 0.1 End state — what the user gets when this ships

A QA engineer has a green Test Cases artifact with local execution enabled (the
existing Docker opt-in). In the Sandbox panel, alongside **Run** and **Check
flaky**, there are now **two** new actions, deliberately separate so the user
chooses when to spend LLM calls:

1. **"Mutation test"** — score only. Cheap-ish (no LLM); just reruns the suite.
2. **"Improve coverage"** — the auto-generate-and-prove loop. Shown after a
   score exists (or runs its own score first). Spends LLM calls.

**Stage 1 — they click "Mutation test":**

```
Mutation score · 78%  ·  killed 31 / 40 mutants  ·  baseline 14/14 passing

Survived (9) — bugs your tests would miss:
  ✗ cart.ts:42   >  →  >=     (boundary not tested)
  ✗ cart.ts:51   +  →  -      (arithmetic not asserted)
  ✗ tax.ts:18    true → false (branch not exercised)
  …
```

**Stage 2 — they click "Improve coverage":**

```
Improving · attempt 2 of 3 · re-scoring…

Mutation score · 78% → 93%  ·  auto-wrote 4 tests killing 6 survivors

✓ cart.ts:42   killed — new test "rejects quantity at boundary"  (attempt 1)
✓ tax.ts:18    killed — new test "applies zero-rate branch"      (attempt 2)
✗ cart.ts:51   still survives — likely needs a human assertion
```

The outcome: an objective quality number the user can act on, and a one-click
path to a **measurably stronger** suite whose new tests are *proven* to catch
bugs the old suite missed. Every improve attempt is a versioned artifact
(`parent_id` chain, exactly like self-heal), and every score is persisted so the
trend is visible over time. Still 100% local — nothing leaves the machine.

## 1. Problem

Line coverage is the only quality signal Tessera exposes today, and it is a weak
one: it proves a line *ran*, not that any assertion would *fail* if that line
were wrong. A generated suite can look 100%-covered and be near-useless. The user
has no way to know which of their passing tests are load-bearing and which are
theatre — and no fast way to fix the gaps once found.

## 2. Approach

### Stage 1 — score (one PR)

A bounded loop on top of `sandbox_service::run`:

1. **Baseline.** Run the suite once. If it is not all-green, abort with a clear
   message — mutation scoring against a red suite is meaningless (a "survivor"
   can't be told from a pre-existing failure). Capture coverage from the baseline
   `RunnerOutput.coverage`.
2. **Generate mutants.** Parse each *source* file (`is_test == false`) with
   `ast_service`, apply the mutation operators (§5.2) to produce a list of
   single-edit **mutants**, each a `(file, byte-range, replacement, descriptor)`.
   Keep only mutants on lines the baseline actually **covered** (uncovered lines
   are guaranteed survivors — scoring them is noise and wasted runs). Cap the
   total (§4).
3. **Run per mutant.** For each mutant, clone the baseline `RunInput`, replace the
   one source file's `contents` with the mutated text, and run the *unchanged*
   tests in the same hardened container.
   - suite **Failed** → mutant **killed** ✅
   - suite **Passed** → mutant **survived** ❌ (a real gap)
   - suite **Error** (didn't compile/run) → mutant **errored**, excluded from the
     score denominator (a mutant that won't build proves nothing).
4. **Score.** `score = killed / (killed + survived)`. Persist the check + per-
   mutant rows. Return the score and the survivor list.

### Stage 2 — improve (second PR, reuses self-heal machinery)

A bounded loop that turns survivors into feedback, exactly mirroring
`healing_service`:

5. Synthesize survivors into one instructive `reviewer_feedback` block:
   *"The test suite did not catch these defects. Add or strengthen test cases so
   each fails when the described change is present:\n- cart.ts:42 — `>` changed to
   `>=` was not detected (add a boundary test)\n- …"*
6. `generation_service::generate` with that feedback + `parent_id = current
   artifact` → a new test-cases version.
7. **Re-baseline.** Run the new suite; it must be green (a regenerated test that
   fails on real code is rejected — don't chase it).
8. **Re-score** (Stage 1) and compare. Keep the **best** version by mutation score.
9. Stop when score == 100%, attempts exhausted, or no improvement; report the lift.

Both stages are cheap to *build* because they compose existing entry points:
`RunInput`/`run` already carry source + tests + coverage; `generate` already takes
`reviewer_feedback` + `parent_id` (the self-heal loop proves this works); the
N-runs-under-one-`CancelToken` shape and the persisted-history pattern both exist
(`run_flaky` + `flaky_check_repo`).

## 3. Scope

### PR 1 — Stage 1 (score + persistence)
In: green-baseline gate; coverage-guided, capped mutant generation for **JS/TS**;
~5 mutation operators (§5.2); per-mutant sandbox run reusing `run`'s preamble;
killed/survived/errored classification; mutation score; survivor list with
file:line + descriptor; streamed progress (`mutant 12/40`); one shared
`CancelToken` (Stop works unchanged); **persisted history** (migration `0009`,
`mutation_check_repo`) + a "Mutation history" trend in the sandbox panel; the
**"Mutation test"** action.

### PR 2 — Stage 2 (improve)
In: survivor → `reviewer_feedback` synthesis; bounded improve loop via `generate`
+ `parent_id`; re-baseline + re-score per attempt; best-by-score selection;
streamed attempt progress; result view showing score lift + which new test killed
which survivor; the separate **"Improve coverage"** action.

### Out (deferred, §7)
Python/Go operators; higher-order mutants (>1 edit); equivalence-mutant detection
(provably-unkillable mutants); focused per-survivor regen instead of whole-
artifact; CLI/Action surfacing; mutation-score CI gate.

## 4. Design decisions

- **Two actions, never one.** Scoring is LLM-free; improving spends tokens.
  Keeping them separate lets the user look at the gaps before paying to fix them
  (mirrors how Run and Check-flaky are distinct). The store carries both a
  `score` slice and an `improve` slice so they coexist with normal-run + flaky.
- **Performance is the dominant constraint** — this is inherently N+1 runs. Three
  defenses, all in PR 1:
  - **Coverage-guided selection** — only mutate covered lines (from the baseline
    `RunnerOutput.coverage`). The single biggest win; turns guaranteed-survivor
    noise into zero runs.
  - **Mutant cap** — `MUT_MAX_MUTANTS` (default 40, clamped [1, 200]); when the
    operator set produces more, sample deterministically and **`log()` the count
    dropped** (no silent truncation — rules: a bounded sweep must say so).
  - **Early-exit per mutant** — a mutant is killed the instant *one* test fails;
    the runner already returns `Failed` on first-failure semantics, so no extra
    work is needed beyond not re-running.
- **New orchestrator service, not a method on `sandbox_service`.** Stage 2 needs
  *both* `sandbox_service::run` and `generation_service::generate`; neither
  sibling may call the other (layering, rules §4.2). A new
  `services/mutation_service.rs` holds both dep bundles and owns both loops —
  exactly how `healing_service` was justified.
- **Reuse `run` and `generate` verbatim.** No signature/prompt/output-schema
  changes. The mutant engine is the only new domain logic.
- **Green-baseline precondition.** Both stages refuse a non-green suite up front
  with `AppError::InvalidInput`. This is the one rule that makes the score
  trustworthy.
- **Errored mutants leave the denominator.** A mutant that fails to compile is
  not evidence about the suite; counting it would deflate the score arbitrarily.
- **No new opt-in gate.** Every inner run goes through `sandbox_service::run`,
  which already enforces the Docker `sandboxOptIn` gate. Mutation is a *run mode*.
- **One shared `CancelToken`** across the whole sweep (every inner `run`) →
  **Stop** kills the current container and ends the check immediately, registered
  under `client_run_id` in the existing `RunRegistry` (Stop works unchanged).
- **Improve attempt bounds:** default 3, clamped [1, 5]; backend re-clamps (UI
  value is a hint, never trusted — mirrors flaky/self-heal).
- **"Best version" = highest mutation score** across improve attempts; ties →
  latest. That version is what the user lands on. Guards against a regeneration
  that kills new survivors but resurrects old ones.

## 5. Where the changes go (point-to-point)

### 5.1 Backend types — `providers/runners/mod.rs` (or a new `mutation` module)
- `enum MutantStatus { Killed, Survived, Errored }` — serde `snake_case`, with
  `as_str` / `from_str_value` like the sibling enums.
- `struct MutationOperator { id: &'static str, .. }` — descriptor for the kind of
  edit (arithmetic / relational / logical / boolean-literal / return-negation).
- `struct Mutant { file: String, line: u32, operator_id: String, original: String,
  replacement: String, byte_start: u32, byte_end: u32 }` (camelCase wire form).
- `struct MutantResult { mutant: Mutant, status: MutantStatus }`.
- `struct MutationResult { score: f64, killed: u32, survived: u32, errored: u32,
  total: u32, baseline_run_id: String, mutants: Vec<MutantResult>,
  dropped_count: u32 }`.
- Stage 2: `struct ImproveAttempt { attempt: u32, artifact_id: String, score: f64,
  killed: u32, survived: u32 }` and `struct ImproveResult { outcome:
  ImproveOutcome, attempts_used: u32, final_artifact_id: String, start_score: f64,
  final_score: f64, attempts: Vec<ImproveAttempt>, error_message: Option<String> }`
  where `ImproveOutcome { Improved, Perfect, Exhausted, NoProgress, Error }`.

### 5.2 Mutant engine — pure module (the new intellectual core, unit-test heavy)
- `fn generate_mutants(parsed: &ParsedFile, source: &str, covered_lines:
  &HashSet<u32>) -> Vec<Mutant>` — pure. Walks the tree-sitter nodes and emits one
  `Mutant` per applicable operator site on a covered line. v1 operators (JS/TS):
  1. **Arithmetic** — `+ - * / %` swapped (`a + b` → `a - b`).
  2. **Relational** — `> >= < <= == !=` swapped / boundary-shifted (`>` → `>=`).
  3. **Logical** — `&&` ↔ `||`.
  4. **Boolean literal** — `true` ↔ `false`.
  5. **Return negation / removal** — negate a boolean return; drop a statement.
- `fn apply_mutant(source: &str, m: &Mutant) -> String` — pure byte-range splice.
- Unit tests: each operator produces the expected edit; no mutants on uncovered
  lines; cap + deterministic sampling; string/comment literals are *not* mutated
  (e.g. `"a + b"` in a string is left alone); empty/uncovered source → no mutants.

### 5.3 Orchestrator — `services/mutation_service.rs` (new)
- `pub async fn score(request: MutationRequest, deps: &SandboxDeps<'_>, on_event:
  Option<MutationSink>) -> AppResult<MutationResult>` — reuses `prepare_run` for
  the opt-in/load/language/`RunInput` preamble, runs the baseline, gates on green,
  generates mutants, loops `run` per mutant under one `CancelToken`, classifies,
  persists, returns. Never calls runner/repo code except through `run` + the repo.
- `pub async fn improve(request: ImproveRequest, gen_deps: &GenerationDeps<'_>,
  sandbox_deps: &SandboxDeps<'_>, on_event: Option<ImproveSink>) ->
  AppResult<ImproveResult>` — Stage 2 loop: `score` → `synth_feedback(survivors)`
  → `generate` → re-baseline → re-`score` → keep best, bounded. Mirrors
  `healing_service::heal` move-for-move.
- `fn synth_feedback(survivors: &[MutantResult]) -> String` — pure, capped length;
  unit-tested (empty / single / many-truncated).
- Tests (no Docker; scripted providers + a `MultiScriptedRunner` like flaky/heal):
  score with all-killed → 100%; mixed kill/survive; errored mutant excluded from
  denominator; non-green baseline aborts; cancel mid-sweep yields cancelled;
  improve raises score across 2 attempts; exhausted keeps best; no-progress bail.

### 5.4 Commands + registration — `commands/sandbox.rs`, `lib.rs`
- `run_mutation_test(...) -> Result<MutationResult, String>` and
  `improve_coverage(...) -> Result<ImproveResult, String>` — thin handlers,
  build the dep bundle(s) as `run_test_sandbox` / `generate_artifact` do,
  `.map_err(|e| e.to_string())`, `#[allow(clippy::needless_pass_by_value)]`.
- `list_mutation_checks` / `get_mutation_check` read-back commands (mirror the
  flaky-history pair).
- Stream progress on `mutation://event` and `improve://event` channels (mirror the
  flaky/generation payload pattern): `{ kind: "mutant", done, total }` /
  `{ kind: "attempt", attempt, score }`.
- Register all four in the `lib.rs` invoke_handler list.

### 5.5 Persistence — migration `0009_mutation_checks.sql` (new, additive)
Mirrors `0008_flaky_checks.sql` exactly (id/created_at/updated_at TEXT, app-minted
UUID PKs, every FK + WHERE/ORDER-BY column indexed, `run_id` FK `ON DELETE SET
NULL` so history outlives a purged run):
- `mutation_checks(id, artifact_id, project_id, baseline_run_id, score, killed,
  survived, errored, total, created_at, updated_at)` — one row per completed score.
- `mutation_check_mutants(id, check_id, file, line, operator_id, status,
  created_at, updated_at)` — one row per mutant; `check_id` FK `ON DELETE CASCADE`.
- `mutation_check_repo` (mirror `flaky_check_repo`): batch insert (no N+1),
  paginated `list`, `get`. History write is **best-effort** — a failure never
  discards the in-memory `MutationResult` (same rule flaky follows).

### 5.6 Zod mirror — `packages/shared/src/schemas/mutation.schema.ts` (new)
- `MutantStatusSchema = z.union([z.literal('killed'), z.literal('survived'),
  z.literal('errored')])`; `ImproveOutcomeSchema` likewise.
- `MutantSchema`, `MutantResultSchema`, `MutationResultSchema`,
  `MutationRequestSchema`, `ImproveAttemptSchema`, `ImproveResultSchema`,
  `ImproveRequestSchema` mirroring the Rust structs (camelCase; optionals
  `.optional()`; `score` a `0..=1` `.refine`). Round-trip contract test in the
  same PR (rules §12.3.1).

### 5.7 IPC wrapper — `apps/desktop/src/lib/ipc/mutation.ts` (new)
- `runMutationTest`, `improveCoverage`, `listMutationChecks`, `getMutationCheck` —
  validate request with the Zod schema, send via `invokeAndParse`, parse the
  result; subscribe to `mutation://event` / `improve://event` for progress. No raw
  `invoke`.

### 5.8 UI — `sandbox-run-panel.tsx` + `sandbox-store.ts`
- Store: a `mutation` slice (score + live `done/total`) and an `improve` slice
  (attempt counter + result), keyed by artifact, separate from run/flaky/heal.
- Panel: two actions gated on the SAME `optIn && runnable` condition — **"Mutation
  test"** (always available) and **"Improve coverage"** (enabled once a score with
  survivors exists), plus a `Max attempts [3]` −/+ stepper on the latter
  (clamped 1–5 UI, backend re-clamps). Helper text on each.
- Results: a `MutationResultView` — score header + `killed/survived/errored`
  chips, a survivor list (`file:line  > → >=  descriptor`), and a collapsible
  **"Mutation history"** trend (reads `list_mutation_checks`, mirrors the flaky
  trend). Stage 2 adds an `ImproveResultView` — `start → final` score lift +
  per-attempt rows + "killed — new test … (attempt N)" badges. UI exempt from
  coverage.

## 6. Verification

- `pnpm --filter @testing-ide/desktop run test:rust` — mutant-engine unit tests
  (each operator, no-uncovered-line, cap/sampling, no-string-mutation),
  `synth_feedback` tests, and the `score` + `improve` orchestrator tests (scripted
  LLM + `MultiScriptedRunner`) pass; no Docker needed.
- `pnpm --filter @testing-ide/desktop run test:frontend` — Zod round-trip contract
  test for the new schemas passes.
- `pnpm typecheck` + `pnpm lint` (ESLint + Clippy `-D warnings`) clean.
- Migration check: `0009` applies on a fresh DB and on top of `0008`; additive
  only (no existing table altered).
- Manual (Docker available, opt-in enabled): generate a JS/TS suite with a weak
  assertion; **"Mutation test"** → confirm a survivor is reported on the
  under-tested line and the score reflects it; **"Improve coverage"** → confirm a
  new test is generated, the suite stays green, the survivor is now killed, and
  the score rises; confirm a survivor that needs a human assertion is reported as
  still-surviving after the attempt bound; Stop mid-sweep kills the container and
  finalizes cleanly; the check appears in "Mutation history".

## 7. Future (separate docs / migrations)

- **Python + Go operators** — the engine is per-grammar; the orchestrator,
  harness, persistence, and UI are language-agnostic and absorb them unchanged.
- **Equivalence-mutant detection** — flag provably-unkillable mutants (e.g.
  `i++` → `i = i + 1`) so they don't unfairly depress the score.
- **Higher-order mutants** — combine edits to find tests that pass only by luck.
- **Focused per-survivor regen** — regenerate only the case(s) targeting a
  survivor via the existing `repair_runnable_files` precedent, instead of the
  whole artifact (cheaper, less disruptive to passing tests).
- **CLI / GitHub Action + score gate** (P0 #3) — `tessera mutate`, a PR check that
  fails below a configurable mutation-score threshold, machine-readable exit code.
