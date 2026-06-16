# Agentic self-healing loop

> Status: **draft** (v2, P0 #1) — design only, not yet built · Owner: core
> Depends on: `generation_service::generate` (LLM entry point) and
> `sandbox_service::run` (execution entry point) — both v1, both reused verbatim.
>
> This composes two existing services into a bounded run → diagnose → regenerate →
> rerun loop. No new prompt, no new LLM-output schema, no migration in the first
> slice. The new code is an orchestrator + a feedback-synthesis helper + the
> command/IPC/UI surface.

## 0. Where this sits in v2

v2's theme (V2_VISION §1): **"from test generator to autonomous test-quality
platform — still 100% local."** The three quality axes v2 adds:

- **Self-healing** (P0 #1, *this doc*) — tests repair themselves on failure.
- **Mutation score** (P0 #2) — does the suite actually catch bugs.
- **Flaky-test detection** (P2 #7, *shipped*) — can a test be trusted twice.

Self-healing is the **repair** axis and the anchor of Phase A. It directly attacks
the #1 research pain (V2_VISION §2): "AI almost right but not quite." A generated
suite proves and fixes itself *before the user ever sees a red test*.

## 0.1 End state — what the user gets when this ships

A QA engineer generates a Test Cases artifact and has local execution enabled (the
existing Docker opt-in). In the Sandbox panel, alongside **Run** and **Check flaky**,
there is now a **"Generate & self-heal"** action (and/or a **"Heal"** button shown
after a failed normal run).

They click it. Tessera runs the suite, and on any failure it feeds the exact
failure output back to the LLM, regenerates the failing artifact, and reruns —
streaming progress ("Attempt 2 of 3 · 1 test still failing"). When it settles they
see:

```
Self-heal · healed in 2 attempts · 14/14 passing

✓ TC-CART-01  adds item to cart            passed
✓ TC-CART-09  computes tax                 passed   (healed — attempt 2)
   └ attempt 1: expected 19.99 to equal 20.00
```

…or, when a test cannot be healed:

```
Self-heal · stopped after 3 attempts · 13/14 passing

✗ TC-CART-07  applies bulk discount        still failing — likely a real source bug
   └ attempt 3: expected 45.00 to equal 50.00
```

The outcome: the artifact the engineer reviews is one that **already passes**, or
the loop has narrowed the failure to a **probable real bug in the source** (the test
couldn't be made to pass), which is itself high-value signal. Every attempt is a
versioned artifact (`parent_id` chain), so the repair history is auditable. Still
100% local — nothing leaves the machine.

This doc specs the **first shippable slice**: whole-artifact regeneration, bounded
retries, in-app surface. Later slices (§7): focused per-case regen, CLI/Action
surfacing, persisted heal history.

## 1. Problem

A generated test suite is "almost right but not quite" — a wrong expected value, a
bad import, an off-by-one assertion. Today the user runs it, sees red, and must
either hand-fix or manually click regenerate with their own feedback. The loop is
real but *manual*. All the parts to automate it already exist; nothing composes
them.

## 2. Approach

Wrap the existing run + generate calls in a bounded loop:

1. Run the suite in the sandbox (`sandbox_service::run`).
2. If all tests pass → done.
3. Otherwise synthesize the per-test `failure_message`s into one `reviewer_feedback`
   string and call `generation_service::generate` with that feedback +
   `parent_id` = the current artifact → a new version.
4. Rerun the new version. Repeat until all green, retries exhausted, or no progress.

This is cheap because it reuses two entry points verbatim:

- `RunResult.tests[].failure_message` and `RunResult.error_message` already carry the
  exact assertion text to feed back.
- `GenerationRequest` already has **`reviewer_feedback: String`** and
  **`parent_id: Option<String>`** — regeneration-from-feedback + version chaining
  exist today; the loop just supplies them programmatically instead of the user.

## 3. Scope (v1 of the feature)

In: bounded run→regen→rerun loop; feedback synthesized from failures; whole-artifact
regeneration; `parent_id` version chain per attempt; stop conditions (all-pass /
max-attempts / no-progress / error); streamed attempt progress; in-app result view
showing healed vs still-failing + which attempt healed each test.

Out (deferred, §7): focused per-failing-case regeneration; persisted heal history /
trend (needs a migration); CLI/Action surfacing; healing across the flaky verdict
(only heal `stable_fail`, never `flaky`).

## 4. Design decisions

- **New orchestrator service, not a method on either existing service.** The loop
  needs *both* `generation_service` and `sandbox_service`; neither sibling may call
  the other (layering, rules §4.2). A new `services/healing_service.rs` holds both
  dep bundles and owns the loop — mirroring how `run_flaky` owns its loop inside one
  service.
- **Reuse `generate` and `run` verbatim.** No changes to their signatures, prompts,
  or output schema in the first slice. The loop is pure composition.
- **Whole-artifact regeneration first.** Regenerate the entire test-cases artifact
  with feedback (zero new generation code). Focused per-case regen — for which
  `generation_service::repair_runnable_files` is the existing precedent — is a §7
  refinement.
- **Attempt bounds:** default max 3 attempts, clamped [1, 5]. Backend re-clamps; the
  UI value is a hint, never trusted (mirrors the flaky / opt-in philosophy).
- **No new opt-in gate.** Each iteration runs through `sandbox_service::run`, which
  already enforces the Docker `sandboxOptIn` gate. Self-heal is a *run mode*, not a
  new permission.
- **One shared `CancelToken`** across the whole loop (every inner `run` + the waits)
  → **Stop** kills the current container and ends the heal immediately.
- **Stop conditions** (first to fire wins):
  1. all tests pass → `healed`.
  2. `attempt == max` → `exhausted` (return the best attempt by pass count).
  3. the failing-test set is identical to the previous attempt → `no_progress`
     (bail early; the model is stuck — don't burn LLM calls).
  4. any `generate` or `run` returns `Err` → abort with `error_message`.
- **"Best attempt" = highest `passed_count`** across attempts; ties → latest. That
  version is what the user lands on.
- **Heal only genuine failures.** Drive the loop off `status == failed` tests. A
  later slice (§7) gates this on the flaky verdict so a `flaky` test is never
  "healed" by chasing noise.

## 5. Where the changes go (point-to-point)

### 5.1 Backend types — `providers/runners/mod.rs` (or a new `healing` module)
- `enum HealOutcome { Healed, Exhausted, NoProgress, Error }` — serde `snake_case`,
  with `as_str` / `from_str_value` like the sibling enums.
- `struct HealAttempt { attempt: u32, artifact_id: String, passed_count: u32,
  failed_count: u32, failures: Vec<HealFailure> }` (`camelCase` wire form).
- `struct HealFailure { name: String, failure_message: Option<String> }` — the
  per-test failure carried forward into the next attempt's feedback (and shown in the
  "attempt N: …" UI line).
- `struct HealResult { outcome: HealOutcome, attempts_used: u32, final_artifact_id:
  String, final_run_id: String, passed_count: u32, failed_count: u32,
  attempts: Vec<HealAttempt>, error_message: Option<String> }`.

### 5.2 Feedback synthesis — pure helper (unit-testable core)
- `fn synth_feedback(failures: &[HealFailure]) -> String` — pure. Formats the failed
  tests into one instructive `reviewer_feedback` block, e.g.
  `"The following generated test cases failed when executed. Fix each failing case so
  it passes against the source under test:\n- TC-CART-09 (computes tax): expected
  19.99 to equal 20.00\n- …"`. Cap the number/length of failures folded in to stay
  within the prompt budget (`generate` already enforces a hard token budget; keep the
  feedback compact). Unit tests: empty, single, many (truncation), missing message.

### 5.3 Orchestrator — `services/healing_service.rs` (new)
- `pub async fn heal(request: HealRequest, gen_deps: &GenerationDeps<'_>,
  sandbox_deps: &SandboxDeps<'_>, mut on_event: Option<HealSink>)
  -> AppResult<HealResult>` where `HealRequest { artifact_id, max_attempts,
  opt_in_confirmed, client_run_id, /* regen params: model, provider, project_id,
  project_name, scope_hint, project_summary */ }`.
- Loop:
  1. one `CancelToken`, registered under `client_run_id` in the existing
     `RunRegistry` (so the existing `cancel_test_sandbox` Stop works unchanged).
  2. `run` the current artifact → `RunResult`. Record a `HealAttempt`.
  3. all-pass → `Healed`, return. Emit progress event each attempt.
  4. compare failing-test set to previous → identical → `NoProgress`, return.
  5. `attempt + 1 >= clamp(max_attempts, 1, 5)` → `Exhausted` (pick best attempt),
     return.
  6. `synth_feedback(failures)` → `generate(GenerationRequest { reviewer_feedback,
     parent_id: current_artifact_id, .. })` → new artifact id becomes current. On
     `Err` from `generate` or `run` → `Error` with `error_message`.
- The function never calls provider or repo code directly — only `generate` / `run`,
  exactly like the existing services delegate downward.
- Tests (no Docker; scripted providers + a `MultiScriptedRunner` like the flaky
  tests): heal-on-2nd-attempt (fail→regen→pass); exhausted after max with best
  attempt chosen by `passed_count`; no-progress bail when failures repeat; a
  `generate` error and a `run` error each abort with `error_message`; cancel mid-loop
  yields a cancelled result.

### 5.4 Command + registration — `commands/healing.rs` (new), `lib.rs`
- `run_self_heal(app, pool, registry, config, crypto, request: HealRequest)
  -> Result<HealResult, String>` — thin handler: build both dep bundles (as
  `generate_artifact` and `run_test_sandbox` do today), call
  `healing_service::heal`, `.map_err(|e| e.to_string())`,
  `#[allow(clippy::needless_pass_by_value)]`.
- Stream attempt progress on a `heal://event` channel (mirror generation's
  `generation://event` payload pattern): `{ kind: "attempt", attempt, passed, failed }`.
- Register `run_self_heal` in the `lib.rs` invoke_handler list.

### 5.5 Zod mirror — `packages/shared/src/schemas/` (new `heal.schema.ts`)
- `HealOutcomeSchema = z.union([z.literal('healed'), z.literal('exhausted'),
  z.literal('no_progress'), z.literal('error')])`.
- `HealFailureSchema`, `HealAttemptSchema`, `HealResultSchema`, `HealRequestSchema`
  mirroring the Rust structs (camelCase; optionals `.optional()`). Round-trip
  contract test in the same PR (rules §12.3.1).

### 5.6 IPC wrapper — `apps/desktop/src/lib/ipc/healing.ts` (new)
- `runSelfHeal(request: HealRequest): Promise<HealResult>` — validate `request` with
  `HealRequestSchema`, send, parse the result with `HealResultSchema` via
  `invokeAndParse`. Subscribe to `heal://event` for progress. No raw `invoke`.

### 5.7 UI — `sandbox-run-panel.tsx` + `sandbox-store.ts`
- Store: add a `heal` slice keyed by artifact (mirror the `flaky` slice) — a
  `healResult: HealResult | null` + phase + live attempt counter, with `startHeal` /
  `attemptHeal` / `finishHeal` / `failHeal`. Separate from normal-run and flaky state
  so all three coexist.
- Panel: a **"Generate & self-heal"** action gated on the SAME `optIn && runnable`
  condition, plus a small `Max attempts: [3]` −/+ stepper (clamped 1–5 in UI; backend
  re-clamps). One line of helper text: "Runs the suite, then feeds failures back to
  the model to fix the failing tests automatically. Bounded retries; stops when all
  pass or it stops improving."
- Results view: a `HealResultView` — top summary ("healed in 2 attempts · 14/14
  passing" / "stopped after 3 · 13/14"), per-test rows reusing `TestRow` chips, a
  "healed — attempt N" badge on tests that flipped to passing, and a collapsible
  per-attempt failure trail (`attempt 1: expected …`). Still-failing tests labelled
  "likely a real source bug". Component-level only (UI exempt from coverage).

## 6. Verification

- `pnpm --filter @testing-ide/desktop run test:rust` — `synth_feedback` unit tests +
  the `heal` orchestrator tests (scripted LLM + `MultiScriptedRunner`) pass; no
  Docker needed.
- `pnpm --filter @testing-ide/desktop run test:frontend` — Zod round-trip contract
  test for the new schemas passes.
- `pnpm typecheck` + `pnpm lint` (ESLint + Clippy `-D warnings`) clean.
- Manual (Docker available, opt-in enabled): generate a JS/TS test-cases artifact
  with a deliberately wrong expected value; click "Generate & self-heal"; confirm the
  loop regenerates, the test flips to passing within the attempt bound, and the
  result view shows "healed — attempt N". Then force an unhealable case (test asserts
  behavior the source genuinely lacks) and confirm it stops at max attempts and
  labels the test a likely real bug. Stop mid-heal kills the container and finalizes
  cleanly.

## 7. Future (separate docs / migrations)

- **Focused per-case regeneration** — regenerate only the failing case(s) instead of
  the whole artifact, using the existing `repair_runnable_files` follow-up-call
  precedent. Cheaper and less disruptive to passing tests.
- **Flaky-aware healing** — run a flaky check first (or reuse a recent one) and heal
  only `stable_fail` tests, never `flaky` ones, so the loop never chases noise.
- **Persisted heal history** — a migration + repo (mirror `flaky_check_repo`) to
  record each heal run and its attempt trail for a trend over time.
- **CLI / GitHub Action surfacing** (P0 #3) — `tessera heal` / a PR check that
  auto-heals generated tests and reports the outcome with a machine-readable exit
  code.
