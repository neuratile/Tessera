# Plan — Closed-Loop Sandboxed Test Runner (Docker, JS/TS first)

> Status: Planned | Owner: TBD | Created: 2026-06-04
> Feature #1 from [FEATURE_REVIEW.md](../docs/FEATURE_REVIEW.md) · ROADMAP "Live test runner with coverage overlay".

## 1. Goal

Close the **generate → run → measure** loop. Take a generated Test Cases artifact,
execute it inside an isolated Docker sandbox on the user's own machine, and paint
pass/fail + line coverage onto the Monaco editor gutters.

**Sandbox decision:** local Docker. Not Daytona/E2B/cloud (those upload code →
break the local-first guarantee; Daytona self-host needs Kubernetes). Cloud
runners may be added *later* as optional plug-ins behind the same trait (§11).

**Language scope:** JavaScript + TypeScript first (one vertical slice, fully
shipped), then a second language (Python) reusing the same pipeline.

## 2. Non-goals (this milestone)

- No Python / other languages yet (Phase 6 follow-up).
- No cloud execution backends.
- No mutation testing (separate roadmap item).
- No editing/saving source from the editor (separate deferred item).
- Default behaviour unchanged: execution is **opt-in, off by default**.

## 3. Core guarantee — must hold

Tessera promises *no code execution and no remote upload on the default path*.
This feature adds local execution, so:

- Execution is **opt-in** via an explicit setting, **off by default**.
- Each run requires the opt-in flag to be on; the backend rejects run requests
  when the flag is off (defence in depth, not just a hidden UI button).
- Code never leaves the machine — runs in a local container, no network.
- If Docker is absent, the feature is shown as unavailable, never silently
  degraded.

## 4. Architecture (fits existing layering)

Mirror the `LlmProvider` pattern: a trait with pluggable impls, one service as
the sole entry point.

```
commands/sandbox.rs            Tauri IPC handler — thin, validate, delegate
services/sandbox_service.rs    Sole entry point — orchestrates a run end to end
providers/runners/mod.rs       TestRunner async trait + shared types
providers/runners/docker_js.rs JS/TS Docker implementation
repositories/test_run_repo.rs  SQL only — persist runs + results
db migration 0004_test_runs.sql
```

Frontend:

```
packages/shared/src/schemas/test-run.schema.ts   Zod contract (FE/BE)
apps/desktop/src/lib/ipc/sandbox.ts               Typed IPC wrapper (Zod-validated)
apps/desktop/src/stores/sandbox-store.ts          Run state
apps/desktop/src/components/...                    Run button + results panel + gutter glue
```

**Flow:** `sandbox_service::run(test_case_id)` → load source + generated test from
DB → build temp workspace → `TestRunner::run(workspace)` (Docker) → parse results
+ coverage → `test_run_repo::insert` → return `RunResult` → FE paints gutters.

### `TestRunner` trait (shape, not final code)

- `async fn run(&self, input: RunInput, cancel: CancellationToken) -> Result<RunResult, RunnerError>`
- `RunInput` = language, source files, generated test file(s), resource limits.
- `RunResult` = per-test status list + coverage report + raw runner stdout/stderr (truncated).
- Keeps `sandbox_service` ignorant of Docker specifics, so a cloud impl can drop in later.

## 5. Data model — migration `0004_test_runs.sql`

- `test_runs` — one row per run.
  - `id`, `artifact_id` (the test-case artifact), `project_id`, `status`
    (`pending` | `running` | `passed` | `failed` | `error` | `cancelled`),
    `runner` (`docker-js`), `started_at`, `finished_at`, `duration_ms`,
    `passed_count`, `failed_count`, `error_message` (nullable).
- `test_run_cases` — one row per individual test assertion.
  - `id`, `run_id` (FK), `name`, `status` (`passed`|`failed`|`skipped`),
    `duration_ms`, `failure_message` (nullable), `source_line` (nullable).
- `test_run_coverage` — coverage per source line.
  - `id`, `run_id` (FK), `file_path`, `line`, `hits`.
- Index `test_run_cases(run_id)`, `test_run_coverage(run_id, file_path)`.
- Parameterized SQL only. No N+1 — batch-insert cases/coverage.

## 6. Contract schemas (`packages/shared`)

Rust serde is source of truth; Zod mirrors it (rules §12.3.1). Add with a
round-trip contract test in `contract-schemas.test.ts`.

- `RunRequest { artifactId: string, optInConfirmed: boolean }`
- `TestResult { name, status: 'passed'|'failed'|'skipped', durationMs, failureMessage?, sourceLine? }`
- `CoverageLine { filePath, line, hits }`
- `RunResult { runId, status, passedCount, failedCount, durationMs, tests: TestResult[], coverage: CoverageLine[], errorMessage? }`
- Status discriminator strings must match the Rust enum exactly.

## 7. Docker runner design (JS/TS)

### Image
- Pre-built runner image `tessera-runner-js` with `vitest` + `c8`/istanbul
  pre-installed (no per-run `npm install` → fast, deterministic, offline).
- Pin base image by digest (e.g. `node:20-alpine@sha256:...`).
- Ship the `Dockerfile`; build locally on first enable, or pull from a registry.
  Decide in Phase 0; default to **local build on first enable** (no registry dep).

### Per-run workspace
- Create a throwaway temp dir.
- Write into it: the source file(s), the generated test file, a minimal
  `package.json`, and a `vitest.config` with coverage enabled.
- Mount temp dir at `/work`; container writes coverage output back there; read
  results after the run, then delete the temp dir.

### Container hardening flags (security gate — §10)
- `--rm` (ephemeral)
- `--network none` (no egress — code can't phone home)
- `--cpus <N>` and `--memory <M>` caps
- `--pids-limit <N>`
- `--read-only` root fs + a small `--tmpfs /tmp`
- `--cap-drop ALL`
- `--security-opt no-new-privileges`
- non-root user inside the image
- wall-clock timeout (tokio) → `docker kill` on expiry
- cancellation token wired through → `docker kill` on user Stop

### Inside the container
- Run `vitest run --coverage --reporter=json` (or equivalent) → emits a test
  results JSON + an istanbul `coverage-final.json`.
- Both files land in `/work`; the host reads and parses them.

## 8. Coverage + result mapping

- **Test results:** parse the vitest JSON reporter → per test: name, status,
  duration, failure message, and source line where available → `TestResult[]`.
- **Coverage:** parse istanbul `coverage-final.json` → `statementMap` + hit
  counts → flatten to `{ filePath, line, hits }[]`. Line with `hits = 0` =
  uncovered.
- Normalize file paths back to the project-relative paths the editor uses.

## 9. Phased build — ship in order, one slice at a time

Each phase = its own branch (`feat/sandbox-...`), green CI, squash-merge. Keep
WIP off master.

### Phase 0 — Decide + ADR (no product code)
- [ ] Write ADR `docs/adr/00XX-sandbox-test-runner.md`: Docker choice, opt-in
      requirement, threat model, image strategy (local build vs registry).
- [ ] Spike: detect Docker presence/version from Rust; decide UX when absent.
- [ ] Confirm base image + digest pin.

### Phase 1 — Contract + schema
- [ ] Add Zod schemas (§6) in `packages/shared/src/schemas/test-run.schema.ts`.
- [ ] Add Rust serde structs mirroring them (in `providers/runners/mod.rs`).
- [ ] Round-trip contract test in `contract-schemas.test.ts`.
- [ ] Migration `0004_test_runs.sql` (§5) + repo skeleton `test_run_repo.rs`.

### Phase 2 — Backend vertical slice (JS/TS), tested with a mock
- [x] Define `TestRunner` trait + types in `providers/runners/mod.rs`.
- [x] Implement `docker_js.rs` (build workspace, run container, parse output).
      Hardening flags applied; full verification + cancellation = Phase 3,
      richer coverage/source-line mapping + fixtures = Phase 4.
- [x] `sandbox_service.rs` orchestration (sole entry point).
- [x] `commands/sandbox.rs` thin IPC handler; register in `lib.rs`.
- [x] `test_run_repo.rs` batch inserts (landed in Phase 1).
- [x] Unit-test the service with a `ScriptedRunner` mock (mirror `ScriptedLlm`
      pattern) — no Docker needed in unit tests.

> **Contract note (gap surfaced in Phase 2 — RESOLVED):** the runner
> consumes a `structured_data.files[]` array (`{ path, contents, isTest }`)
> on the test-cases artifact. The `test_cases_v1` prompt now emits an
> optional `files[]` array (minimal source-under-test + one vitest spec per
> source file) alongside the descriptive cases, so generated artifacts are
> runnable end to end. `files[]` stays optional, so descriptive-only
> generations remain valid; `sandbox_service` still rejects an artifact
> without `files[]` with a clear `INVALID_INPUT` error.

### Phase 3 — Sandbox hardening (SECURITY GATE — blocks merge)
- [x] Apply all container flags in §7 (added `--ulimit fsize`; non-root via
      image `USER` in `docker/Dockerfile.runner-js`).
- [x] Wall-clock timeout + cancellation token → `docker kill`. `--name` +
      explicit `docker kill` on both timeout and cancel; `kill_on_drop(true)`
      backstop. `CancelToken` plumbed through the `TestRunner` trait (UI Stop
      wiring lands in Phase 5).
- [x] Docker-absent / daemon-down handled with a typed error + clear UX
      (`ensure_docker_available` → `RunnerError::DockerUnavailable`).
- [x] Security review run on the diff; findings recorded in
      `docs/adr/0004-sandbox-test-runner.md`.
- [x] Integration test behind a `docker`-gated flag — `#[ignore]`d
      `docker_runner_executes_a_real_suite` (skips in CI without Docker).
- [x] Hardened input/output guards: workspace file-count + total-byte caps
      (`RunInput::validate`); per-field caps on runner-supplied test names /
      failure messages before DB persistence.

### Phase 4 — Coverage parse + storage
- [x] Parse istanbul coverage + vitest results (§8). Coverage now dedupes
      multiple statements per `(file, line)` taking max hits; vitest results
      carry `source_line` from the reporter `location` (config gains
      `includeTaskLocation: true`).
- [x] Persist cases + coverage; expose via `RunResult` (landed Phase 2 via
      `test_run_repo`; source-line now populated end to end).
- [x] Unit-test parsers against captured fixture JSON (no Docker) —
      `fixtures/vitest-report.json` + `fixtures/istanbul-coverage.json`.
- Branch coverage deferred: `CoverageLine` models line hits only; adding
  branches is an FE/BE contract change (own slice), out of this phase.

### Phase 5 — Frontend
- [x] Opt-in setting (off by default) in settings UI; persisted (`ui-store`
      `sandboxOptIn`, localStorage).
- [x] `lib/ipc/sandbox.ts` typed wrapper (Zod-validated, no raw `invoke`) —
      `runTestSandbox` + `cancelTestSandbox`.
- [x] `stores/sandbox-store.ts` run state (idle/running/done/error), keyed by
      artifact id.
- [x] **Run** button on the Test Cases artifact view (`SandboxRunPanel`,
      disabled unless opt-in on). Docker-absence surfaces as an `error`
      `RunResult` rather than a disabled button (clear message either way).
- [x] Monaco gutter decorations: green = covered, amber = uncovered (matched
      to the open file by path suffix). Per-file failing-line red markers are
      deferred — `TestResult` carries a source line but not a file path, so a
      failing assertion can't yet be mapped to a specific source file without
      a contract change; failures show their line textually in the panel.
- [x] Results panel: X/Y passed, per-test failures + source lines, Stop button.
      Functional Stop via a `clientRunId` the backend keys the cancel token on
      (the run IPC is blocking, so the UI must know the id up front).

### Phase 6 — Tests, docs, polish
- [x] Playwright E2E: opt-in → generate test-cases → run → see pass/fail in the
      results panel (Tauri IPC mocked in `e2e-tauri-mocks.ts`; `run_test_sandbox`
      + `cancel_test_sandbox` scripted). Gutter pixels aren't asserted (Monaco
      canvas), but the run path + results render are.
- [x] Update ROADMAP (moved from "planned" to "shipped (JS/TS)"); plan checkboxes.
- [x] Tracing spans around build/run/parse stages (`sandbox_run` span +
      build/run/parse debug events in `docker_js`).

## 10. Security checklist (must pass before master)

- [x] Execution off by default; backend rejects runs when opt-in flag is off.
- [x] `--network none` applied (verified end to end by the docker-gated test).
- [x] CPU / memory / pids / timeout limits enforced (+ `--ulimit fsize`).
- [x] `--cap-drop ALL`, `no-new-privileges`, non-root, read-only rootfs.
- [x] Temp workspace always cleaned up (even on error/cancel) — `WorkspaceGuard`.
- [x] Generated test + source paths validated — no path traversal into the host
      (+ file-count / total-size caps).
- [x] Raw stdout/stderr truncated before storage; runner-supplied test names /
      failure messages capped before DB persistence (no unbounded blobs).
- [x] Security review clean — findings + resolutions in ADR-0004.

## 11. Future (out of scope here, design for it now)

- **Python runner** — `providers/runners/docker_py.rs` (pytest + coverage.py),
  same trait, same service, same tables. Proves the abstraction.
- **Cloud runners** — optional `TestRunner` impls (E2B / Daytona) for users with
  no local Docker, gated behind a separate explicit opt-in. AGPL/upload concerns
  apply only to users who choose them.
- **Mutation testing** + **coverage trend over runs** build on these tables.

## 12. Acceptance criteria (definition of done for JS/TS slice)

1. With opt-in on and Docker present: clicking Run on a generated JS/TS test
   case executes it, shows "X/Y passed", and paints gutters within a few seconds.
2. With opt-in off OR Docker absent: Run is unavailable; backend refuses any run.
3. A failing test shows the failure message and marks the assertion line red.
4. Coverage gutters distinguish covered vs uncovered lines.
5. Stop cancels a running container; temp workspace is removed.
6. No network access from inside the container (verified in the security gate).
7. CI green: typecheck, lint, Rust + FE tests, clippy. Docker integration test
   gated so it skips where Docker is unavailable.
