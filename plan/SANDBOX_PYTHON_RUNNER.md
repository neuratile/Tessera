# Plan — Python Sandbox Runner (`docker_py`)

> Status: Implemented (PR pending review) | Owner: TBD | Created: 2026-06-10
>
> Implementation notes (2026-06-10):
> - Both phases landed in one PR (`feat/sandbox-python-runner`).
> - §7 naming refinement: the prompt mandates a **double underscore**
>   between the snake-cased case id and the description
>   (`test_tc_login_01__rejects_empty_password`) — a single underscore
>   leaves the id/description boundary ambiguous in snake case. The
>   `docker_py` parser splits on `__`, uppercases / re-hyphenates the id
>   token, and falls back to the raw nodeid for non-conforming names.
> - Phase 2 step 3 (golden live-Ollama → sandbox e2e) deferred: the
>   integration-test CI job is non-blocking/flaky and provisions no Docker
>   images; the deterministic docker-gated `docker_py_runner_executes`
>   test covers the runner end to end, and the prompt is snapshot-locked.
> - Phase 2 step 5 (demo recording) is a manual follow-up.
> Second vertical slice of the sandboxed test runner — Python (pytest + coverage.py)
> behind the same `TestRunner` trait. Follows [`SANDBOX_TEST_RUNNER.md`](./SANDBOX_TEST_RUNNER.md)
> (JS/TS slice, shipped) and the security model in
> [ADR-0004](../apps/desktop/src-tauri/docs/adr/0004-sandbox-test-runner.md).
> ROADMAP: "Python (`docker_py`) + cloud runners next, behind the same `TestRunner` trait".

## 1. Goal

Run generated **Python** test cases in the local Docker sandbox and paint
pass/fail + line coverage in the editor — exactly what the JS/TS slice does
today. This is the cheapest credible expansion of the runner: the expensive
parts (security gate, service orchestration, data model, schemas, UI) shipped
with the first slice and are reused untouched.

Why now (marketing): the "verification debt" narrative (96% of devs don't
fully trust AI-generated code) is the project's best-performing angle, and the
target audience (QA automation, entry-level, AI/ML devs) is Python-first.
"Tessera now runs the AI tests it generates — Python too" is the next post,
and "we added a language in one file because the trait held" is the
contributor on-ramp for Java/Go runners.

Secondary goal: **prove the `TestRunner` abstraction** with a second
implementation, and extract the Docker hardening into a shared harness so
every future runner inherits the same security flags by construction.

## 2. Non-goals (this milestone)

- **No third-party Python deps inside the sandbox** — stdlib + pytest only.
  A generated test that imports `requests` etc. fails with a clear
  `ModuleNotFoundError` surfaced in the results panel (network is off by
  design, so `pip install` inside the container is impossible anyway).
  Curated extra-deps image is a possible follow-up, not this slice.
- No branch coverage — `CoverageLine` stays line-hits-only (same deferral as
  the JS slice).
- No cloud runners, no other languages (Java/Go are explicitly invited as
  contributions once this lands).
- No prompt-schema changes — the test-cases artifact contract
  (`files[] { path, contents, isTest }`) is byte-identical.
- Default behaviour unchanged: execution stays **opt-in, off by default**.

## 3. Core guarantee — must hold (unchanged from JS slice)

- Execution **opt-in** via the existing setting; backend rejects runs when the
  flag is off (`sandbox_service.rs:126-129` — no change needed).
- Code never leaves the machine — local container, `--network none`.
- Runner image is **built locally** from a checked-in Dockerfile (pinned base
  digest, pinned tool versions) — never pulled as a prebuilt app image.
- If Docker is absent, the feature surfaces as unavailable, never silently
  degraded.

## 4. Architecture

### Reused untouched (zero changes)

| Piece | Why it just works |
|---|---|
| `sandbox_service.rs` flow (load artifact → validate → run → persist) | Language-agnostic; only runner selection changes (§4.2) |
| `RunInput::validate` (path traversal, file/byte caps) | Operates on paths/bytes, not language |
| Opt-in rejection (defence in depth) | Already enforced before runner dispatch |
| Tables `test_runs` / `test_run_cases` / `test_run_coverage` (migration 0004) | `runner` column is open TEXT — `'docker-py'` needs **no migration** |
| Zod contract (`test-run.schema.ts`) + `lib/ipc/sandbox.ts` | No language field crosses IPC; `RunResult` shape identical |
| Frontend: Run button, results panel, Stop, coverage gutters | Consume `RunResult` only |

### New / modified

```
apps/desktop/src-tauri/
  docker/Dockerfile.runner-py             new — pinned python:3.12-slim digest +
                                           pinned pytest, pytest-json-report, coverage; non-root user
  src/providers/runners/docker_harness.rs new — shared Docker plumbing extracted from docker_js.rs:
                                           hardening-flag builder, docker_kill, WorkspaceGuard,
                                           output-truncation constants, image-presence check
  src/providers/runners/docker_py.rs      new — TestRunner impl: write workspace → run container →
                                           parse pytest + coverage.py JSON → RunResult
  src/providers/runners/docker_js.rs      modify — consume docker_harness (pure refactor, behaviour identical)
  src/providers/runners/mod.rs            modify — RunnerLanguage gains Python; from_path() maps .py
  src/commands/sandbox.rs                 modify — replace hardcoded Arc::new(DockerJsRunner::new())
                                           with per-language selection (§4.2)
  src/prompts/test_cases_v2.rs            modify — language-conditional runnable-files instruction (§7)
  fixtures/pytest-report.json             new — captured pytest-json-report output
  fixtures/coverage-py.json               new — captured `coverage json` output
.github/workflows/ci.yml                  modify — build/cache tessera-runner-py, run ignored py test (§9)
```

### 4.1 Shared harness extraction — the safety mechanism

The hardening flags currently live inline in `docker_js.rs:302-313`. Copying
them into `docker_py.rs` invites silent drift — one runner shipping with a
weaker sandbox. Instead extract `docker_harness.rs`:

- `fn hardened_run_args(image: &str, workspace: &Path, limits: &ResourceLimits) -> Vec<String>`
  — emits the one canonical flag set: `--network none`, `--cpus`, `--memory`,
  `--pids-limit`, `--ulimit fsize`, `--read-only`, `--tmpfs /tmp`,
  `--cap-drop ALL`, `--security-opt no-new-privileges`, `/work` mount.
- `docker_kill(container_name)` — used by the existing
  `tokio::select!` (complete / cancel / timeout) pattern, which moves here too.
- `WorkspaceGuard` (RAII temp-workspace cleanup) — moved from `docker_js.rs`.
- Truncation constants (`MAX_OUTPUT_BYTES` 64 KB, `MAX_TEST_NAME_BYTES` 512,
  `MAX_FAILURE_MSG_BYTES` 8 KB) and workspace caps (200 files / 8 MB).
- A unit test asserting the flag set contains every required hardening flag —
  the drift tripwire.

This is a **pure refactor** for the JS path: existing `docker_js` unit tests
and the ignored Docker integration test must pass unchanged.

### 4.2 Runner selection

`commands/sandbox.rs:40` currently constructs `DockerJsRunner` unconditionally.
Replace with selection on the language already detected from the artifact's
`files[]` extensions (`sandbox_service.rs:367-372`,
`RunnerLanguage::from_path`):

- `JavaScript | TypeScript` → `DockerJsRunner` (`tessera-runner-js`)
- `Python` → `DockerPyRunner` (`tessera-runner-py`)

Smallest shape that works: a `match` in `sandbox_service` (or a tiny
`runners::factory` mirroring `providers/factory.rs`) returning
`Arc<dyn TestRunner>`. Mixed-language `files[]` is rejected with a clear
`InvalidInput` error ("test cases mix Python and JS/TS sources").

## 5. Docker image — `Dockerfile.runner-py`

Mirrors `Dockerfile.runner-js` conventions:

- Base: `python:3.12-slim@sha256:<pinned digest>`.
- `pip install --no-cache-dir pytest==8.x pytest-json-report==1.5.x coverage==7.x`
  (exact pins chosen at implementation time; pinned, not floating).
- Non-root user (uid 1000), `WORKDIR /work`.
- Built locally on first enable / on demand, cached by Dockerfile hash in CI —
  same lifecycle as `tessera-runner-js`.

## 6. Container invocation + result extraction

Workspace mounted at `/work` (only writable path besides `--tmpfs /tmp`,
rootfs read-only). Single container command:

```
COVERAGE_FILE=/work/.coverage \
coverage run -m pytest /work --json-report --json-report-file=/work/results.json -q ; \
coverage json -o /work/coverage/coverage.json
```

- `coverage json` runs even when tests fail (`;` not `&&`) — failed runs still
  report coverage, matching JS behaviour.
- Results read from the mounted workspace after exit, same as the JS runner
  reads `results.json` + `coverage/coverage-final.json`.
- Exit code is informational only; status derives from parsed results
  (`derive_status` pattern: any failure → Failed, any pass → Passed,
  empty → Error).

### Parsers (in `docker_py.rs`, unit-tested against fixtures — no Docker)

- `parse_pytest_results()` — pytest-json-report `tests[]`: `nodeid` → test
  name, `outcome` (`passed`/`failed`/`skipped`) → `TestStatus`,
  `call.duration` (secs, f64) → `duration_ms`, `call.crash.message` (truncated)
  → `failure_message`, `lineno` → `source_line`.
- `parse_coverage_py()` — `coverage json` per-file `executed_lines` /
  `missing_lines` → `CoverageLine { file_path, line, hits }` with **hits = 1
  for executed, 0 for missing** (coverage.py reports executed/missing, not hit
  counts; existing FE only distinguishes `hits == 0` vs `> 0`, so gutters are
  unaffected).
- Fixtures: `fixtures/pytest-report.json`, `fixtures/coverage-py.json` —
  captured once from a real run, committed.

## 7. Prompt change — `test_cases_v2.rs`

Today the runnable-files instruction mandates **vitest** specs
(`test_cases_v2.rs:45-55`). Make that block language-conditional on the source
language of the supplied chunks:

- JS/TS sources → unchanged vitest instruction.
- Python sources → emit `test_<module>.py` pytest files marked `isTest: true`;
  each test function name carries the owning case id as its first token after
  `test_`, lower-snake-cased: `def test_tc_login_01_rejects_empty_password():`
  (TC-id extraction in the parser uppercases/re-hyphenates to match `TC-…`).
- Schema (`files[] { path, contents, isTest }`) unchanged → `VERSION` stays
  `test_cases_v2`; insta snapshots regenerated and diff-reviewed in the PR.

## 8. Phases (2)

### Phase 1 — Backend slice (`feat/sandbox-python-runner`)

1. Extract `docker_harness.rs`; refit `docker_js.rs` onto it. Pure refactor —
   all existing runner tests green before anything Python lands.
2. `RunnerLanguage::Python` + `from_path(".py")`; runner selection per §4.2;
   mixed-language rejection + unit test.
3. `Dockerfile.runner-py` (pinned digest + pins).
4. `docker_py.rs`: workspace → container → parse, reusing harness; parsers +
   fixtures + unit tests (status mapping, duration conversion, truncation,
   executed/missing → hits, TC-id extraction).
5. `#[ignore]`d Docker integration test `docker_py_runner_executes` (mirror of
   `docker_runner_executes`): tiny module + passing/failing pytest file →
   asserts statuses + coverage lines + network absence.

**Exit:** `cargo test` + clippy pedantic green; JS path behaviour identical;
ignored py test passes locally with Docker.

### Phase 2 — Prompt, CI, end-to-end (`feat/sandbox-python-prompt-ci`)

1. Prompt conditional (§7) + insta snapshot regen.
2. CI: extend `sandbox-runner-test` job — build/cache `tessera-runner-py` by
   Dockerfile hash (same tar-cache pattern), run both ignored tests.
3. Golden end-to-end check: generate test cases for a small sample Python
   module via live Ollama (`test:integration` harness), run them in the
   sandbox, assert a `RunResult` with coverage.
4. Docs: README runner section, ROADMAP row (Python → shipped), CLAUDE.md
   sandbox paragraph, this plan's checkboxes.
5. Record the generate → run → coverage-gutters demo on a Python file — the
   launch-post asset.

**Exit:** CI green incl. both docker-gated tests; full Python flow works in
the app; demo captured.

## 9. Security checklist (must pass before master)

Identical bar to the JS slice (§10 there); the shared harness is the
mechanism, not a promise:

- [ ] Execution off by default; backend rejects runs when opt-in flag is off
      (existing — covered by existing tests, re-verified for the py path).
- [ ] All hardening flags emitted by `docker_harness::hardened_run_args` and
      asserted by a unit test (network none, cap-drop ALL, no-new-privileges,
      non-root, read-only rootfs, cpu/mem/pids/fsize caps).
- [ ] Wall-clock timeout + Stop → `docker kill` verified for `docker_py`.
- [ ] Workspace always cleaned (WorkspaceGuard) — including error/cancel.
- [ ] Path traversal + file-count/byte caps enforced (existing
      `RunInput::validate`, unchanged).
- [ ] Runner-supplied names/messages truncated before persistence.
- [ ] No network from inside the container verified by the docker-gated py
      test (socket connect attempt fails).
- [ ] Security review of the diff; findings + resolutions appended to
      ADR-0004 (amendment section, not a new ADR — model unchanged).

## 10. Acceptance criteria

1. Opt-in on + Docker present: Run on a generated Python test-cases artifact
   executes pytest, shows "X/Y passed", paints coverage gutters on `.py`
   files.
2. JS/TS slice behaviour byte-identical (refactor-only changes there).
3. Opt-in off or Docker absent: both runners unavailable; backend refuses.
4. A Python test importing an unavailable third-party module fails with a
   readable `ModuleNotFoundError` in the results panel — no hang, no crash.
5. Mixed-language artifacts rejected with a clear message.
6. CI green: typecheck, lint, Rust + FE tests, clippy, both docker-gated
   runner tests (skipped where Docker unavailable).

## 11. Future (out of scope, design holds the door open)

- Curated "batteries" image variant with vetted common deps (requests, numpy)
  — separate opt-in, separate Dockerfile.
- Java / Go runners — contributor-facing: one `docker_<lang>.rs` + one
  Dockerfile against the harness. Write a CONTRIBUTING pointer once this
  merges.
- Cloud runners (E2B / Daytona) — separate explicit opt-in, same trait.
- Branch coverage — FE/BE contract change, own slice.
