# AI-first staged review: first-release contract

> Status: **planned**, not implemented. Target: v2; contract version: 1.
> Source: [#105](https://github.com/neuratile/Tessera/issues/105), under
> [roadmap #104](https://github.com/neuratile/Tessera/issues/104).

## Outcome and scope

**Review my changes** explains what could break in staged changes, cites the
reviewed source, and optionally tests a specific finding. This contract defines
the first review release ("v1" below), not the older shipped v1 release line.
It takes priority over earlier v2 priorities for this workflow. The first reviewed
and runnable slice is JS/TS; existing artifact generation and its Python runner
remain available. CLI, MCP, automatic fixes, background reviews,
unstaged review, and arbitrary test commands are outside this release.

The user selects a Git repository and explicitly starts a review. Scope is the
**index compared with HEAD**: read staged Git blobs, never substitute working-tree
files. A partially staged file uses only its staged version. Show "Staged changes
only; unstaged edits and untracked files are not reviewed" before starting and
alongside results. List omitted paths and reasons; never call a partial review
an approval of the whole repository.

| Repository condition | Required behavior |
|---|---|
| Unborn HEAD | Compare the index with an empty tree; omit `baseCommit`; treat staged files as additions. |
| Clean index | Complete without a model call: `outcome: "no_changes"`, empty findings. |
| Non-Git folder or invalid repository | Reject before queueing with `INVALID_INPUT`; suggest opening a Git repository. |
| Unmerged index entries or unreadable required blob | Reject with `INVALID_INPUT` or `IO_ERROR`; no fabricated snapshot. Intent-to-add without staged content is excluded. |
| Rename | Use old/new paths and blobs. Recognize exact-content renames; represent other moves as delete/add in v1, without guessing identity. |
| Delete | Retain the old path/content and old-side line references; new path/content are absent. Additions have the inverse shape. |
| Unsupported file | Exclude binary/non-UTF-8 content, symlinks, submodules, unsafe paths, and extensions outside `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`; record a reason. Python and other extensions are outside this first review slice. No following links or fetching submodules. |
| Only excluded changes | Complete without a model call: `outcome: "nothing_reviewable"`; show exclusions, not "no bugs found". |

Honor existing ignored/sensitive-file rules and secret redaction. Repository text
is untrusted data, including instructions inside comments. Do not execute Git
hooks, external diff/textconv helpers, package scripts, or repository commands.

## Immutable source and freshness

Capture the base commit and index entries once, using Git object IDs with their
object format (`sha1` or `sha256`). Store the exact blob bytes needed for review,
context, and later execution, with SHA-256 content hashes; never recapture a file
from disk to fill a missing snapshot. Retain bytes locally subject to existing
storage protections; do not put source or credentials into logs.

`snapshotId` is a UUID identifying an immutable stored record, not a claim that
different captures are identical. Its content identity is the tuple of Git object
format, optional `baseCommit`, `indexFingerprint`, and a sorted manifest of
`{ side, path, mode, contentHash }` for every included old/new/context blob, sorted
by `(side, path)` with duplicate references to the same side/path deduplicated.
Context uses `side: "new"`; old and new entries for one path remain distinct. A hash is
lowercase SHA-256 hex over raw bytes, before redaction or newline normalization.
The index fingerprint additionally detects changes to excluded paths and file modes.
Persist excluded paths/reasons and the actual context selection alongside it.

Capture is read-only: do not run `git write-tree`, write Git objects, or alter the
index. `indexFingerprint` is SHA-256 of UTF-8 compact JSON
`[1, gitObjectFormat, entries]`, where each entry is
`[pathBase64, mode, stage, blobId]`. Encode raw Git path bytes as standard padded
base64; use six-digit mode strings, integer stage, lowercase hex blob IDs; sort
entries by raw path bytes then stage. Include all index entries, even excluded
paths, and accept only stage 0. This versioned format requires no stored Git tree.

Paths are repository-relative, `/` separated, case-preserving, with no drive,
absolute path, NUL, or `..` traversal. Reject non-UTF-8 Git paths in v1 with an
explicit exclusion. Each changed-file entry records `changeKind`
(`added`, `modified`, `deleted`, `renamed`), available old/new path, Git blob ID,
mode and content hash. Absent sides are omitted, not replaced with empty text.

A source reference is `{ snapshotId, path, side, lineStart, lineEnd }`:
`side` is `old` (base) or `new` (index); lines are 1-based, inclusive, with
`lineEnd >= lineStart`. Validate against that exact blob. Deleted lines cite
`old`; added lines cite `new`; rename references use the corresponding side's
path. File-level metadata findings omit both line fields. No invented locations.
Unchanged supporting files use their index blob and `side: "new"`.

Model-facing transformations must preserve raw-source line numbers: redact secret
characters in place without adding/removing line breaks; CRLF-to-LF conversion
may change bytes but must preserve logical lines. Label context excerpts with their
original starting line, never renumber them from one. If safe redaction cannot
preserve line layout, exclude that file with `redaction_unmappable`; do not send
the secret or accept references in transformed coordinates.

Compare HEAD identity and the index fingerprint before and after capture; if either
changes, reject with `INVALID_INPUT` and ask for a new review. Once captured,
later edits never rewrite the record. Recheck before display refresh and before
execution; expose `freshness: "current" | "stale" | "unknown"` independently of
review status. Changed HEAD/index means stale; failed freshness checks mean
unknown. Unstaged-only edits do not make the staged snapshot stale. Never label
an old run as evidence for new code. Execution on stale/unknown snapshots is
blocked in v1; request a new review. No background resnapshot or silent retry.

## Lifecycle, findings, and evidence

| Review status | Meaning and transitions |
|---|---|
| `queued` | Validated request saved; may become running, failed, cancelled, or interrupted. |
| `running` | Capturing, collecting context, or generating findings; becomes completed, failed, cancelled, or interrupted. |
| `completed` | Valid final review saved; outcome is `reviewed`, `no_changes`, or `nothing_reviewable`. Findings may be empty. |
| `failed` | Required step failed or exceeded a hard limit; save safe error and incomplete scope. |
| `cancelled` | User stopped the task; cancel model work and any active sandbox container. |
| `interrupted` | On restart, previously queued/running work has no live worker. Retain partial history; do not resume automatically. |

Terminal reviews are immutable; an explicit retry creates a new `reviewId`.
Persist step progress before emitting events. Unvalidated streamed text is only
a preview, never a finding or final result. A later optional test run has its own
lifecycle and does not change a completed review into running again.

Each finding has a UUID, summary, explanation, expected behavior, severity
(`critical`, `major`, `minor`, `trivial`), validated source references, and evidence:

| `evidenceState` | Meaning |
|---|---|
| `suspected` | Static reasoning supports a risk; no qualifying execution evidence. |
| `reproduced` | An identified assertion demonstrates the stated behavioral violation on this source snapshot and test version. |
| `inconclusive` | An attempted check cannot establish the claim: pass, skip, missing dependency, invalid test, timeout, cancellation, or infrastructure failure. |

A failing process alone does not reproduce a bug. Show expected versus observed
behavior and the specific assertion. A passing test alone never proves the
application fixed or the finding false. A clean review means "no findings in the
reviewed scope," not bug-free software. Keep each evidence attempt append-only;
later inconclusive attempts do not erase earlier reproduction evidence.

## Wire and provenance contract

New DTOs use **camelCase fields**, UUID identifiers, and lowercase/snake_case
state literals, matching existing generation and runner serde conventions.
Use existing provider literals (`ollama`, `ollama-cloud`, `openai`, `openrouter`,
`anthropic`, `gemini`). Optional fields are omitted. Future Rust DTOs are the
source of truth; matching Zod schemas and malformed-input/round-trip tests must
land together. This document adds no runtime schemas or command registrations.

Proposed `start_review` accepts `{ args: { projectId, scope: "staged", providerConfigId,
model } }` and returns `{ reviewId, status: "queued" }`. The backend resolves the
project root from `projectId`, validates the provider/model, and atomically pins
the explicitly active provider configuration. A missing/inactive selection is
`INVALID_INPUT`; never pick the first row or fall back to another provider.
Local Ollama remains the default. Cloud selection retains existing source-sharing
disclosure and redaction. Snapshot credentials in memory only, never in records.

Proposed `review://event` payloads contain `reviewId`, increasing integer
`sequence`, `status`, and `step` (`capture`, `context`, `findings`, `persist`).
Events are progress hints; `get_review` retrieves the authoritative saved result
after missed events/restart. `cancel_review` accepts `reviewId`; repeat cancellation
is harmless and never rewrites an already terminal state.

Every model-backed review stores `providerConfigId`, provider kind, exact model,
`promptVersion`, and input/output token usage. Every test proposal stores
`testId`, positive integer `testVersion`, test content hash, `findingId`,
`snapshotId`, and its own generating provider/model/prompt provenance. Every run
stores `runId` plus those exact bindings, runner kind, image ID/digest, timestamps,
limits, test results and output truncation. Never infer provenance from current
settings. Editing/regenerating a test creates a new version and new run; it cannot
overwrite the original failure or silently weaken the original expected behavior.

Use existing `RunStatus` values (`pending`, `running`, `passed`, `failed`,
`error`, `cancelled`) for execution, not review states. An interrupted active run
is recovered as `error` with interruption detail. Preserve existing `RunResult`
fields and wrap/link them with review provenance rather than changing their meaning.
Execution materializes source from the stored snapshot and approved test version
only. Keep explicit `optInConfirmed: true` plus the backend settings check on
every run, Docker-only execution, `--network none`, no dependency installation or
network fallback, existing path checks and resource limits. Missing dependencies
produce inconclusive evidence. Changing provider settings mid-review affects only
future work; it cannot switch the provider used by an existing task.

IPC remains `Result<T, String>`: successful payloads are direct objects and rejected
commands carry safe strings, as in `commands/generation.rs` and `commands/sandbox.rs`.
Do not introduce an incompatible `{ ok, data }` envelope. Domain errors retain
existing `AppError`/`RunnerError` codes; a persisted review failure adds
`error: { code, message }` with a sanitized message, not raw command stderr.
Use `INVALID_INPUT` for invalid scope/stale execution, `LIMIT_EXCEEDED` for review
ceilings, existing `LLM_*` and `RUNNER_*` codes for provider/runner failures.
Paths in findings are relative; errors contain no absolute paths, SQL, or secrets.

## First-release ceilings

These are proposed review limits, not claims about current implementation.
Lower existing provider/runner limits always win. Bounds apply before allocations
or provider calls where possible; no silent shrinking of changed code.

| Resource | Ceiling | When exceeded |
|---|---|---|
| Changed index entries | 50, including excluded paths | Reject capture with `LIMIT_EXCEEDED`; ask user to stage a smaller change. |
| Source blob | 128 KiB per old/new/supporting blob | Exclude path with `file_too_large`; omit both sides of a changed file. |
| Captured source | 1 MiB total distinct old/new/supporting blobs | Fail capture; never report completion over silently dropped changed files. |
| Supporting context | 20 additional index files, within source byte caps | Omit excess supporting files deterministically, record `context_limit`. |
| Prompt input | 8,000 tokens including instructions/schema; also fit model window minus output reserve | Drop lowest-ranked supporting context first and record omissions. If required changed content still does not fit, fail with `LIMIT_EXCEEDED`. |
| Model output | 6,000 tokens and 128 KiB per response; at most 20 findings | Stop/validate bounded response; overflow, token cutoff, or invalid JSON fails the review. No partial JSON promoted to findings. |
| Review wall time | 300 seconds from queued, including all calls | Cancel outstanding work, fail with `LIMIT_EXCEEDED`; explicit user retry only. |
| Model attempts | One findings call, no automatic repair/retry; test generation is a separate requested operation with the same input/output caps and a 120-second deadline | Surface the failure; do not silently spend more tokens. |
| Sandbox | Existing 200 files / 8 MiB workspace; 60 seconds, 1 CPU, 512 MiB, 256 PIDs; 64 KiB captured output per existing harness | Preserve runner enforcement; timeout/error is inconclusive. Mark clipped output explicitly. |

Persist `excludedFiles` entries (`path`, `reason`), and `truncation` entries
(`resource`, `reason`, `omittedCount`) when selection/output is shortened.
Exclusions may make a completed review partial; display them beside findings.
Do not truncate a source blob and then use its shortened lines as full-file
evidence. If runner output loses the necessary assertion evidence, that attempt
is inconclusive even if the process exited with a failure.

## Checkout examples

The companion fixture is planned in [#113](https://github.com/neuratile/Tessera/issues/113):
`evals/fixtures/checkout/{baseline,regression,clean}/src/checkout.mjs`, materialized
in a temporary Git repository as `src/checkout.mjs`. Static fixture metadata and
fixture test results are not real Tessera review/run records.

Baseline `checkoutTotal(quantity)` uses a fixed 1200-cent unit price and rejects quantities below one.
The staged regression replaces `quantity < 1` with `quantity === 0` on line 7, so
`checkoutTotal(-1)` returns a negative total while zero remains rejected. An unstaged fix must not hide
that staged regression. The immutable behavior assertion expects a rejection;
only a run against captured source demonstrating acceptance can reproduce it.
The clean comparator preserves validation and should yield zero findings.

Illustrative JSON (hash placeholders below are explanatory, not valid production
hashes; fixture evaluation computes real hashes and Git IDs):

```json
{
  "args": {
    "projectId": "00000000-0000-4000-8000-000000000010",
    "scope": "staged",
    "providerConfigId": "00000000-0000-4000-8000-000000000011",
    "model": "qwen2.5-coder:7b"
  }
}
```

```json
{
  "reviewId": "00000000-0000-4000-8000-000000000012",
  "status": "completed",
  "outcome": "reviewed",
  "scope": "staged",
  "freshness": "current",
  "snapshot": {
    "snapshotId": "00000000-0000-4000-8000-000000000013",
    "gitObjectFormat": "sha1",
    "baseCommit": "<base commit object ID>",
    "indexFingerprint": "<canonical index entries SHA-256>",
    "files": [{
      "changeKind": "modified",
      "old": { "path": "src/checkout.mjs", "blobId": "<base blob ID>", "mode": "100644", "contentHash": "<base SHA-256>" },
      "new": { "path": "src/checkout.mjs", "blobId": "<index blob ID>", "mode": "100644", "contentHash": "<index SHA-256>" }
    }]
  },
  "provenance": {
    "providerConfigId": "00000000-0000-4000-8000-000000000011",
    "provider": "ollama",
    "model": "qwen2.5-coder:7b",
    "promptVersion": "staged-review-v1",
    "usageInputTokens": 900,
    "usageOutputTokens": 180
  },
  "findings": [{
    "findingId": "00000000-0000-4000-8000-000000000014",
    "summary": "Checkout accepts negative quantities",
    "explanation": "The staged guard no longer rejects quantities below one.",
    "expectedBehavior": "Reject quantity -1 instead of returning a negative total.",
    "severity": "major",
    "evidenceState": "suspected",
    "sources": [{ "snapshotId": "00000000-0000-4000-8000-000000000013", "path": "src/checkout.mjs", "side": "new", "lineStart": 7, "lineEnd": 7 }]
  }],
  "excludedFiles": [],
  "truncation": [],
  "runs": []
}
```

For the staged clean change, produce a new review/snapshot with that change's
actual hashes and `status: "completed"`, `outcome: "reviewed"`, `findings: []`,
`runs: []`. This differs from an empty index (`outcome: "no_changes"`). No run
is invented for either example; token counts above illustrate shape only.

## Delivery dependencies and acceptance evidence

| Work | Depends on | Evidence before completion |
|---|---|---|
| #106 Git capture; #107 persistence/contracts; #113 fixture | #105 | Git edge cases above; mirrored Rust/Zod boundary tests; seeded bug and clean control. |
| #108 bounded context | #106 | Only captured index/base bytes used; exclusions and token limits tested. |
| #109 findings | #107, #108 | Validated references, malformed/model-specific output tests, selected-provider invariants. |
| #110 coordinator/IPC | #107, #109 | Cancellation, timeouts, restart interruption, event/result consistency. |
| #111 desktop entry/results | #110 | Staged disclosure, progress, errors, accessible evidence and exclusions. |
| #112 execution evidence | #106, #107, #109, #110; sandbox #95 | Source/test bindings, opt-in/no-network limits, stale run rejection, no false reproduction. |
| #100 freshness | #106, #107, #108 | Backend invalidation against captured source/context; stale evidence disclosure. |
| #114 evaluation; #115 walkthrough | #111, #112, #113 | Known-bug recall, clean-control false positives, test validity, time/cost; reproducible first review. |
| #22 E2E; #24 accessibility | #111, #112, #100 | Success, failure, cancellation, stale result and keyboard/screen-reader flows. |
| #116 core extraction; #117 CLI; #118 MCP | Proven desktop workflow, then #116, then shared operations | Reuse engine contracts and permissions; no parallel implementation. |

This documentation issue is complete after contract consistency review. Runtime
schema/IPC tests belong to their implementation issues; no artificial prose test
suite is required here.
