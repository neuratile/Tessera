# ADR-0004: Sandboxed test-runner — Docker choice, threat model, security gate

- **Status**: Accepted
- **Date**: 2026-06-05
- **Authors**: Backend / Sandbox (security gate, Phase 3)
- **Supersedes**: none
- **Superseded by**: none

## Context

Tessera's core promise (`plan/SANDBOX_TEST_RUNNER.md` §3) is **no code
execution and no remote upload on the default path**. The closed-loop test
runner deliberately adds *local* code execution: it takes a generated
test-case artifact, runs it, and paints pass/fail + coverage onto the editor.
This is the single biggest new attack surface in the product, so it ships
behind an explicit security gate (Phase 3) before it can run real code.

## Decision

### Execution backend — local Docker

Local Docker only. Not Daytona / E2B / cloud: those upload code, which breaks
the local-first guarantee; Daytona self-host needs Kubernetes. Cloud runners
may be added later as optional, separately-opted-in `TestRunner` impls behind
the same trait (§11). If Docker is absent the feature is reported unavailable
(`RunnerError::DockerUnavailable`), never silently degraded.

### Opt-in, off by default

Execution is opt-in. Every run carries `optInConfirmed`; the **backend**
rejects a run when it is false (`sandbox_service::run`), so the gate is
defence in depth, not just a hidden UI button. Note: the flag is a per-request
client boolean — for a local desktop app the real trust boundary is the local
machine; this is documented, not a server-side audited setting.

### Threat model

- **Adversary**: untrusted code inside a generated test artifact (an LLM can
  be prompted/poisoned into emitting hostile test code).
- **Assets**: the host filesystem outside the workspace, host network, host
  CPU/memory/disk, and the local SQLite DB / renderer.
- **In scope**: container escape attempts, path traversal out of the
  workspace, network egress (exfiltration / phone-home), resource exhaustion
  (CPU, memory, pids, disk), and unbounded/hostile data flowing back into the
  DB and UI.
- **Out of scope**: a compromised host OS, a malicious local user with shell
  access (the throwaway workspace lives under the per-user temp root), and
  Docker daemon vulnerabilities themselves.

### Container hardening (`providers/runners/docker_js.rs`)

Applied at `docker run` (§7/§10):

- `--network none` — no egress; code cannot phone home.
- `--cpus`, `--memory`, `--pids-limit` — CPU / memory / process caps.
- `--ulimit fsize` — caps the largest file the suite can write into the
  bind-mounted workspace, bounding a disk-fill DoS through `/work`.
- `--read-only` root filesystem + a small `--tmpfs /tmp`.
- `--cap-drop ALL` and `--security-opt no-new-privileges`.
- **Non-root user** supplied by the image (`USER node` in
  `docker/Dockerfile.runner-js`) — a container escape lands unprivileged.
- **Termination** — the wall-clock timeout *and* user cancellation both issue
  an explicit `docker kill <name>` against the container. This is the key
  fix from the security review: dropping the `docker run` child kills only the
  CLI, **not** the daemon-side container, so the timeout was previously
  non-functional. `--rm` reaps the killed container; `kill_on_drop(true)` is a
  backstop for the leaked CLI handle.

### Input / output guards

- Workspace paths validated (`RunInput::validate` + `is_safe_relative_path`):
  no absolute paths, no Windows drive prefixes, no `..` traversal, no empty
  components.
- Workspace size bounded: `MAX_WORKSPACE_FILES` (200) and `MAX_WORKSPACE_BYTES`
  (8 MiB) reject a runaway artifact before the container starts.
- The throwaway workspace is removed on every path (happy, `?`-early-return,
  panic) via the `WorkspaceGuard` RAII drop.
- Captured stdout/stderr truncated to 64 KiB; parsed test names and failure
  messages — written by the *untrusted* container into `results.json` — are
  capped (`MAX_TEST_NAME_BYTES`, `MAX_FAILURE_MSG_BYTES`) before they reach
  the DB. The frontend (Phase 5) must render these as text, never HTML.

## Security review findings (security gate)

Findings from the review of the Phase 2 slice and their resolution in Phase 3:

| # | Severity | Finding | Resolution |
|---|----------|---------|------------|
| 1 | High | Wall-clock timeout did not stop the daemon-side container; the runaway ran unbounded. | `--name` + explicit `docker kill` on timeout **and** cancel; `kill_on_drop(true)` backstop. |
| 2 | Medium | Container ran as root. | Non-root `USER` baked into the runner image; workspace made writable so the non-root user can write results back (also fixes a root-owned-file cleanup leak). |
| 3 | Medium | Read-write `/work` bind mount had no size cap → host disk-fill DoS. | `--ulimit fsize` write cap. |
| 4 | Medium | Attacker-controlled test names / failure messages persisted unbounded. | Per-field byte caps at parse time; flagged for text-only rendering in the UI phase. |
| 5 | Low | No cap on file count / total workspace size. | `MAX_WORKSPACE_FILES` + `MAX_WORKSPACE_BYTES` in `RunInput::validate`. |
| 6 | Low | Runner base image unpinned. | Dockerfile documents digest-pinning the base before distribution. |

## Consequences

- The timeout / cancellation path requires a Docker host to exercise
  end to end. It is covered by an `#[ignore]`d integration test
  (`docker_runner_executes_a_real_suite`) that skips in CI and runs locally
  with `cargo test -- --ignored`.
- Cancellation is plumbed through the `TestRunner` trait (`CancelToken`) but
  fired only by the timeout today; Phase 5 wires the UI Stop button to a
  per-run registry that triggers it.
- The runner image is built locally on first enable (no registry dependency).
  Pin the base image by digest before any pre-built image is distributed.

## Follow-ups

- Phase 4: richer istanbul/vitest mapping (source lines, branch coverage)
  against captured fixtures.
- Phase 5: opt-in setting UI, typed IPC wrapper, run store, Stop button →
  `CancelToken::cancel`, Monaco gutter decorations. Render runner-supplied
  strings as text.
- Consider mounting the source read-only and extracting results via
  `docker cp` to remove host bind-mount writes entirely.
