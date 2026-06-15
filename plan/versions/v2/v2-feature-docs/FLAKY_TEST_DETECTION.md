# Flaky-test detection

> Status: **shipped** (v2, P2 #7) — first slice (§3) implemented · Owner: core
> Depends on: the opt-in Docker sandbox runner (v1 — SANDBOX_TEST_RUNNER.md,
> SANDBOX_PYTHON_RUNNER.md). Reuses its hardened harness verbatim.
>
> Implemented on branch `feat/flaky-test-detection`: backend `TestVerdict` /
> `FlakyTestResult` / `FlakyRunResult` + the pure `aggregate_flaky` (mod.rs),
> `sandbox_service::run_flaky` (shared preamble extracted from `run`), the
> `run_test_sandbox_flaky` command, the Zod mirror + round-trip contract test,
> the `runTestSandboxFlaky` IPC wrapper, and the "Check flaky" UI (runs stepper
> + `FlakyResultView`).
>
> **Persisted flaky history** (§7, first hardening item) shipped on branch
> `feat/flaky-history`: migration `0008_flaky_checks.sql` (`flaky_checks` +
> `flaky_check_tests`, additive — no existing table touched), `flaky_check_repo`
> (transactional `insert_check`, `list_checks`, `fetch_check`), best-effort
> persistence inside `run_flaky`, the `list_flaky_checks` / `get_flaky_check`
> commands + service pass-throughs + `FlakyCheckSummary` / `FlakyCheckRecord`
> types, their Zod mirrors + contract tests, the `listFlakyChecks` /
> `getFlakyCheck` IPC wrappers, and the collapsible "Flaky history" trend in the
> sandbox panel. The remaining **Future items (§7)** — CLI/Action surfacing,
> auto-quarantine, cross-run coverage — stay deferred.

## 0. Where this sits in v2

v2's theme (V2_VISION §1) is **"from test generator to autonomous test-quality
platform — still 100% local."** v1 closed the generate → run → measure loop; v2
makes the suite *prove its own quality*. The quality signals v2 adds are:

- **Self-healing** (P0 #1) — tests repair themselves on failure.
- **Mutation score** (P0 #2) — does the suite actually catch bugs, not just cover lines.
- **Flaky-test detection** (P2 #7, *this doc*) — can each test even be *trusted* to
  give the same answer twice.

Flaky detection is the **trust** axis of that quality story and the cheapest to
ship — it reuses the v1 sandbox harness wholesale. It is the **first v2 feature to
get a per-feature design doc**, and the template for the rest.

## 0.1 End state — what the user gets when this ships

A QA engineer has generated a Test Cases artifact and enabled local execution
(the existing Docker opt-in). In the Sandbox panel they now see, alongside **Run**:

- a **"Check flaky"** button and a small **`Runs: [5]` −/+ stepper** (adjustable 2–20).

They click **Check flaky**. Tessera runs the *whole suite 5 times* back-to-back in
the same hardened, network-less container, streaming progress ("Run 3 of 5…").
When it finishes they see:

```
Flaky check · 5 runs · 2 of 14 tests flaky

⚠ TC-LOGIN-03  rejects expired token        flaky      passed 3/5
✓ TC-LOGIN-01  accepts valid credentials    stable     5/5
✗ TC-CART-07   applies bulk discount        fails      0/5   (real bug)
⚠ TC-CART-09   computes tax                 flaky      passed 4/5
   └ sample failure: expected 19.99 to equal 20.00
```

Each test carries a one-glance verdict: a **green ✓ stable** test they can trust, a
**red ✗ stable-fail** that's a genuine reproducible bug to fix, or a **⚠ flaky** test
with its "passed X/N" score and one captured failure message showing *why* it
sometimes breaks. A top summary ("2 of 14 tests flaky") tells them at a glance how
much of the suite is untrustworthy. **Stop** kills the whole check (and its
container) immediately.

The outcome: before the engineer relies on a green suite — in review, in CI, or as
the basis for the self-healing loop (P0 #1) — they know exactly which tests are
solid and which lie. That is the "trust" guarantee v2 is built to give, delivered
entirely on the user's machine with no code leaving it.

This doc specs the **first shippable slice** of that end state (the full in-app
experience above, minus persisted cross-check history — see §3 / §7). Later slices
surface the same verdict through the headless CLI + GitHub Action (P0 #3) so a PR
check can fail on newly-flaky tests, and persist flakiness over time for trends.

## 1. Problem

A flaky test passes sometimes and fails sometimes on **unchanged** code (timing,
randomness, shared state, dates). It is the worst failure mode: a green run can't
be trusted and a red run might be noise. Test *maintenance* (stale/flaky), not
writing, is the #1 grind in the v2 research (V2_VISION §2). A single sandbox run
cannot detect this — one run yields one verdict.

## 2. Approach

Run the **same** test suite N times back-to-back in the existing hardened sandbox,
then classify each test by comparing its outcome across the N runs:

- all N pass  → `stable_pass` (trustworthy)
- all N fail  → `stable_fail` (a real, reproducible failure)
- mixed       → `flaky` (unreliable — flag it)

N defaults to 5, user-adjustable 2–20. The "5" is internal to one check — the user
clicks once, the suite runs 5 times, and each test gets ONE verdict + a "passed
X/N" score. Re-running the check is a fresh, independent answer (no averaging,
no persisted history in v1).

This is cheap because it composes the existing runner: no new container, no new
parsers — a loop + a tally on top of `sandbox_service`.

## 3. Scope (v1 of the feature)

In: N-run loop, per-test verdict + ratio, sample failure message, adjustable N,
UI badge + summary, iteration #1 persisted as a normal run row.

Out (deferred): cross-check flaky history / trend (needs a migration), coverage
aggregation across runs, parallel runs, auto-quarantine of flaky tests.

## 4. Design decisions

- **No new opt-in gate.** Reuses the Docker `sandboxOptIn` safety gate. Same
  container, same code — flaky check is a *run mode*, not a new permission.
- **Sequential runs.** Parallel Docker contention would skew timing-based flakiness.
- **One shared `CancelToken`** across all N iterations → Stop kills the whole check.
- **Iteration bounds:** default 5, min 2, max 20. Backend re-clamps `runs.clamp(2, 20)`;
  the UI value is a hint, never trusted (mirrors the opt-in gate philosophy).
- **Skipped outcomes** count toward neither pass nor fail; ratio is
  `pass_count / executed_count`. A test skipped in all runs → `stable_pass`, not flaky.
- **No DB migration.** Persist iteration #1 via the existing `persist_success` path so
  it appears in normal run history; compute the flaky verdict across all N in memory.
- **Error/cancel policy:** any iteration `Err` aborts the loop early; return a
  `FlakyRunResult` with `error_message`, no verdict.

## 5. Where the changes go (point-to-point)

### 5.1 Backend types + aggregation — `providers/runners/mod.rs`
- `enum TestVerdict { StablePass, StableFail, Flaky }` — serde `snake_case`
  (`stable_pass` / `stable_fail` / `flaky`), with `as_str` / `from_str_value` like the
  sibling enums.
- `struct FlakyTestResult { name, verdict, pass_count: u32, executed_count: u32,
  total_runs: u32, sample_failure: Option<String> }` (`camelCase` wire form;
  `sample_failure` omitted when `None`).
- `struct FlakyRunResult { run_id, total_runs: u32, flaky_count: u32, non_flaky_count: u32,
  tests: Vec<FlakyTestResult>, error_message: Option<String> }`. `non_flaky_count`
  (not `stable_count`) is every non-flaky test — both `stable_pass` and
  `stable_fail` — so it cannot be misread as "reliably passing".
- `fn aggregate_flaky(outputs: &[RunnerOutput], total_runs: u32) -> Vec<FlakyTestResult>`
  — **pure**, the unit-testable core. Group `TestResult` by `name`, tally
  pass/fail/skip, derive verdict + first failure message.
- Unit tests: verdict boundaries (all-pass, all-fail, 1-of-N flip, all-skip),
  + serde round-trip for `TestVerdict` (mirror the existing enum tests).

### 5.2 Service — `services/sandbox_service.rs`
- Extract the shared preamble of `run()` (opt-in gate → load+typecheck artifact →
  `build_run_input` → select runner) into a private helper so `run()` and the new
  `run_flaky()` share it (avoids duplicating the 4 steps).
- `pub async fn run_flaky(request: RunRequest, runs: u32, deps: &SandboxDeps<'_>)
  -> AppResult<FlakyRunResult>`:
  1. shared preamble → `(input, runner, artifact, case_ids)`.
  2. clamp `runs` to [2, 20]; create one `CancelToken`.
  3. loop `runs` times: `runner.run(input.clone(), cancel.clone())`; collect
     `Vec<RunnerOutput>`. On the first `Err`, stop and return a `FlakyRunResult`
     carrying `error_message` (cancel → cancelled message).
  4. persist iteration #1 via the existing `persist_success` → real `run_id` + history.
  5. `aggregate_flaky(&outputs, runs)`; count flaky/stable; attach `run_id`; return.
- Tests: add a `MultiScriptedRunner` (a `VecDeque<Scripted>`, pops one per `run()`
  call) because the existing `ScriptedRunner` panics on a 2nd call. Cover: a flaky
  test detected across N scripted outputs; an all-stable suite; an iteration error
  aborts the loop with `error_message`.

### 5.3 Command + registration — `commands/sandbox.rs`, `lib.rs`
- `run_test_sandbox_flaky(pool, registry, crypto, request: RunRequest, runs: u32)
  -> Result<FlakyRunResult, String>` — thin handler mirroring `run_test_sandbox`,
  `.map_err(|e| e.to_string())`, `#[allow(clippy::needless_pass_by_value)]`.
- Register in the `lib.rs` invoke_handler list next to `run_test_sandbox`.

### 5.4 Zod mirror — `packages/shared/src/schemas/test-run.schema.ts`
- `TestVerdictSchema = z.union([z.literal('stable_pass'), z.literal('stable_fail'),
  z.literal('flaky')])`.
- `FlakyTestResultSchema`, `FlakyRunResultSchema` mirroring the Rust structs (camelCase;
  optionals `.optional()`). Add a round-trip contract test in the same PR (rules §12.3.1).

### 5.5 IPC wrapper — `apps/desktop/src/lib/ipc/sandbox.ts`
- `runTestSandboxFlaky(args: RunRequest, runs: number): Promise<FlakyRunResult>` —
  validate `args` with `RunRequestSchema`, send `{ request, runs }`, parse the result
  with `FlakyRunResultSchema` via `invokeAndParse`. No raw `invoke`.

### 5.6 UI — `sandbox-run-panel.tsx` + `sandbox-store.ts`
- Store: add a `flaky` slice keyed by artifact (mirror `ArtifactRunState`) — a
  `flakyResult: FlakyRunResult | null` + phase, with `startFlaky` / `finishFlaky` /
  `failFlaky`. Keep it separate from the normal-run state so both can coexist.
- Panel: add a **"Check flaky"** secondary action next to Run, gated on the SAME
  `optIn && runnable` condition, plus a small `Runs: [5]` −/+ stepper (clamped 2–20
  in the UI; backend re-clamps). One line of helper text: "Runs the suite N times to
  catch tests that pass sometimes and fail sometimes. More runs = more confidence,
  slower."
- Results view: a `FlakyResultView` — top summary "X of Y tests flaky", and per-test
  rows showing a ⚠️ `flaky` chip + "passed 3/5", `stable_fail` rows marked as real
  failures, `stable_pass` unmarked. Reuse the chip/pill styles already in
  `RunResultView` / `TestRow`. Component-level only (UI exempt from coverage).

## 6. Verification

- `pnpm --filter @testing-ide/desktop run test:rust` — `aggregate_flaky` unit tests +
  the `run_flaky` service tests (with `MultiScriptedRunner`) pass; no Docker needed.
- `pnpm --filter @testing-ide/desktop run test:frontend` — Zod round-trip contract
  test for the new schemas passes.
- `pnpm typecheck` + `pnpm lint` (ESLint + Clippy `-D warnings`) clean.
- Manual (Docker available, opt-in enabled): generate a JS/TS test-cases artifact,
  click "Check flaky" with N=5; confirm stable tests show 5/5 and a deliberately
  non-deterministic test (e.g. `Math.random() > 0.5`) is flagged flaky with a ratio
  between 1/5 and 4/5. Stop mid-check kills the container and finalizes cleanly.

## 7. Future (separate docs / migrations)

- ~~Persisted flaky history + trend over time (needs a migration / new table).~~
  **Shipped** — see §8.
- Auto-quarantine: tag flaky cases in the sidecar so CI / the CLI can skip or warn.
- Surface the flaky verdict through the headless CLI + GitHub Action (P0 #3) as a
  machine-readable check.

## 8. Persisted flaky history (shipped)

### End state — what the user gets

After running a flaky check, the sandbox panel grows a collapsible **"Flaky
history"** section listing every past check for that artifact, newest first:

```
Flaky history
▸ 2 of 12 flaky    5 runs    Jun 15, 2026, 10:30 AM
▸ 0 of 12 flaky    5 runs    Jun 14, 2026,  4:02 PM
▾ 1 of 12 flaky    8 runs    Jun 13, 2026,  9:10 AM
    ⚠ TC-CART-09 computes tax    flaky    passed 7/8
      └ expected 19.99 to equal 20.00
```

So the engineer can see at a glance whether a suite is *getting* flakier or
settling down, and expand any past check to see exactly which tests were
unreliable then — the verdict is no longer thrown away when the panel closes.
Still 100% local: nothing leaves the machine.

### Design

- **Additive migration `0008_flaky_checks.sql`** — `flaky_checks` (one header
  row per completed check) + `flaky_check_tests` (one row per test verdict). No
  existing table is altered, so it is fully backward compatible with the v1 run
  tables (0004). `flaky_checks.run_id` references the iteration-#1 run with
  `ON DELETE SET NULL`, so purging a run never deletes the historical check —
  history outlives any one run; artifact/project FKs cascade.
- **`flaky_check_repo`** owns all SQL: `insert_check` writes the header + all
  per-test rows in one transaction (batch idiom, no N+1); `list_checks`
  (newest-first, limit re-clamped to [1, 200]) and `fetch_check` read it back.
- **Best-effort persistence in `run_flaky`** — the aggregate is recorded after
  the verdict is computed; a history-write failure is logged and swallowed so it
  can never discard the in-memory result the user sees (same philosophy as the
  name→id bridge). Only the success path persists; an errored/cancelled check
  writes no history row.
- **`list_flaky_checks` / `get_flaky_check`** commands → thin
  `sandbox_service::{list_flaky_history, get_flaky_check}` pass-throughs → repo.
  Mirrored by `FlakyCheckSummary` / `FlakyCheckRecord` (Rust + Zod with
  round-trip contract tests) and the `listFlakyChecks` / `getFlakyCheck` IPC
  wrappers.
- **UI** — the panel fetches history on mount and after each completed check;
  `FlakyHistorySection` renders the trend and lazily fetches a check's per-test
  detail on expand, re-using the live-check `FlakyRow`.

Still out: CLI/Action surfacing, auto-quarantine, cross-run coverage (§7).
