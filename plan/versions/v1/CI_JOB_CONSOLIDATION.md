# Plan: CI Job Consolidation + Branch-Protection Required Checks

> Design record: implementation status varies by section. See [current status](../../../docs/PROJECT_STATUS.md) and the [active roadmap](../../ROADMAP.md) before choosing new work.

## Context

**Goal.** Cut billed CI minutes by collapsing jobs that share an identical
toolchain setup (each job currently re-pays checkout + pnpm install + Rust
toolchain/cache + apt Tauri deps), and — in the same change — wire the
resulting jobs into the `master` ruleset as **required status checks** so CI
must be green before a squash-merge.

**Current state (verified 2026-06-09).**

- `master` has **no classic branch protection** (`/branches/master/protection`
  → 404). Protection is a **repository ruleset** `Protect master`
  (id `17259460`, enforcement `active`) with three rules: `deletion`,
  `non_fast_forward`, and `pull_request` (`allowed_merge_methods: ["squash"]`,
  `required_approving_review_count: 0`, no thread-resolution requirement).
- **There is no `required_status_checks` rule.** Consequences:
  1. Renaming/merging CI jobs does **not** wedge open PRs — no check name is
     referenced by protection. (The earlier CI-cleanup PR avoided renames out
     of caution; that caution is now known to be unnecessary.)
  2. A PR can squash-merge with **red or pending CI** — PR #66 merged while
     `integration-test` was still pending. CI is advisory today, not a gate.

- **`ci.yml` jobs today (9):** `conflict-marker-check`, `lint`, `typecheck`,
  `unit-test`, `build-check`, `server-check`, `e2e-test`,
  `sandbox-runner-test`, `integration-test` (the last is
  `continue-on-error: true`).

**Toolchain profile per job** (what each setup pays for):

| Job | node+pnpm | Rust + tauri apt | other |
|-----|:--:|:--:|--|
| conflict-marker-check | — | — | checkout only (~3s) |
| typecheck | ✓ | — | — |
| build-check | ✓ | — | vite build |
| lint | ✓ | ✓ | eslint + clippy (desktop) |
| unit-test | ✓ | ✓ | `pnpm test` = vitest + `cargo test --lib` |
| e2e-test | ✓ | — | Playwright Chromium |
| server-check | — | Rust only (apps/server workspace) | clippy + test |
| sandbox-runner-test | — | ✓ | Docker image + `--ignored` test |
| integration-test | ✓ | ✓ | live Ollama service, continue-on-error |

**Decisions locked.**
- Only merge jobs with an **identical** toolchain profile, so no job inherits
  setup it doesn't need. Two clean pairs qualify:
  - **Node-only:** `typecheck` + `build-check`.
  - **Node + Rust + tauri-apt:** `lint` + `unit-test`.
- **Do not** merge `e2e-test` (adds Playwright), `server-check` (separate
  `apps/server` rust-cache workspace), `sandbox-runner-test` (Docker, ignored
  test), or `integration-test` (flaky, continue-on-error). Each is its own
  profile; folding them in would waste setup or couple unrelated signal.
- Net: **9 jobs → 7 jobs**, removing 2 redundant setup cycles per CI run.
- The `master` ruleset gains a `required_status_checks` rule listing the 7
  blocking jobs. `integration-test` is **excluded** (continue-on-error / known
  flaky per the integration-test advisory) so it never blocks a merge.

**Trade-off (accepted).** Merging coarsens signal — a merged job reports one
pass/fail instead of two, and runs its steps sequentially (slightly higher
wall-clock per job, lower total billed minutes). Worth it for the two
identical-profile pairs; not worth it elsewhere.

## Approach

Two phases: (1) rewrite `ci.yml` to the 7-job shape; (2) add the
`required_status_checks` rule to the ruleset via `gh api`, matching the new job
names exactly. Validate on the implementing PR itself, then merge.

---

### Phase 1 — Consolidate `ci.yml` (9 → 7 jobs)

**File:** `.github/workflows/ci.yml`

1. **Merge `typecheck` + `build-check` → `frontend-checks`** (node-only).
   - Single job: checkout → pnpm/action-setup → setup-node (`cache: pnpm`) →
     `pnpm install --frozen-lockfile` → step `pnpm typecheck` → step
     `pnpm --filter @testing-ide/desktop run vite:build`.
   - typecheck runs first; if it fails the build step is skipped (acceptable —
     a type error almost always breaks the bundle too). Keep both as **named
     steps** so the failing one is obvious in the logs.

2. **Merge `lint` + `unit-test` → `lint-and-test`** (node + Rust + tauri-apt).
   - Single job: checkout → `./.github/actions/linux-tauri-deps` →
     pnpm/action-setup → setup-node (`cache: pnpm`) →
     `dtolnay/rust-toolchain@stable` (with `clippy`) →
     `Swatinem/rust-cache@v2` (workspace `apps/desktop/src-tauri`) →
     `pnpm install` → steps: `pnpm lint`, `cargo clippy … -D warnings`,
     `pnpm test`.
   - Order: lint/clippy (fast static gates) before `pnpm test` (slow) so a
     style break fails fast. One shared rust-cache restore instead of two.
   - `timeout-minutes`: take the max of the two originals (30).

3. **Keep unchanged:** `conflict-marker-check`, `server-check`, `e2e-test`,
   `sandbox-runner-test`, `integration-test`.

4. **Fix `integration-test.needs`** — currently `needs: [lint, typecheck,
   unit-test]`. Those three names no longer exist. Replace with
   `needs: [lint-and-test, frontend-checks]`.

5. Update the job-header comments to describe the merged scope (the existing
   comments explain why each job is Ubuntu-only; preserve that reasoning).

**Resulting 7 jobs:** `conflict-marker-check`, `frontend-checks`,
`lint-and-test`, `server-check`, `e2e-test`, `sandbox-runner-test`,
`integration-test`.

### Phase 2 — Add required status checks to the `master` ruleset

Add a `required_status_checks` rule to ruleset `17259460` listing the 6
blocking jobs (every new job **except** `integration-test`). A `gh api PUT`
replaces the ruleset's `rules` array, so it must re-send the existing three
rules plus the new one.

**Exact call** (run after Phase 1 is merged so the check contexts already exist
on `master` — GitHub validates loosely, but matching reality avoids a stuck
first PR):

```bash
gh api --method PUT "repos/neuratile/Tessera/rulesets/17259460" \
  --input - <<'JSON'
{
  "name": "Protect master",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    {
      "type": "pull_request",
      "parameters": {
        "allowed_merge_methods": ["squash"],
        "dismiss_stale_reviews_on_push": false,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_approving_review_count": 0,
        "required_review_thread_resolution": false
      }
    },
    {
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": false,
        "do_not_enforce_on_create": false,
        "required_status_checks": [
          { "context": "conflict-marker-check" },
          { "context": "frontend-checks" },
          { "context": "lint-and-test" },
          { "context": "server-check" },
          { "context": "e2e-test" },
          { "context": "sandbox-runner-test" }
        ]
      }
    }
  ]
}
JSON
```

Notes:
- **First** fetch the live ruleset (`gh api repos/neuratile/Tessera/rulesets/17259460`)
  and diff against the payload above before PUT — the `conditions` block must
  match what's already there (the snapshot showed only the three rules, not the
  `conditions`; copy the real value rather than assuming `~DEFAULT_BRANCH`).
- `strict_required_status_checks_policy: false` — do **not** force branches to
  be up-to-date with master before merge (avoids serialized update-merge-rerun
  churn on a solo-maintainer repo). Flip to `true` later if desired.
- `integration-test` is intentionally absent — it is `continue-on-error` and
  chronically flaky; requiring it would block merges for runner-resource
  reasons unrelated to code.

---

## What is NOT changing
- No change to `release.yml`, `auto-merge.yml`, or the
  `linux-tauri-deps` composite action.
- No change to the `deletion`, `non_fast_forward`, or `pull_request` rules
  (squash-only + linear history stay).
- `server-check`, `e2e-test`, `sandbox-runner-test`, `integration-test` job
  definitions are untouched.

## Files touched (summary)
- `.github/workflows/ci.yml` — merge two job pairs, fix `integration-test.needs`.
- Ruleset `17259460` (via `gh api`, not a file) — add `required_status_checks`.

## Verification

1. **YAML parse:** `python -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`.
2. **Open the implementing PR.** Confirm exactly these checks report:
   `conflict-marker-check`, `frontend-checks`, `lint-and-test`, `server-check`,
   `e2e-test`, `sandbox-runner-test` (+ `integration-test` advisory). Both
   merged jobs must run **every** step of their predecessors (verify
   `frontend-checks` shows both typecheck and vite-build; `lint-and-test` shows
   eslint, clippy, and `pnpm test`).
3. **After Phase 2 PUT:** re-fetch the ruleset and assert the
   `required_status_checks` rule is present with the 6 contexts. Open a throwaway
   PR with a deliberately failing test → confirm merge is **blocked** until the
   check passes (proves the gate now bites, unlike today).
4. **Billed-minutes sanity:** compare total runner-minutes for a CI run
   before/after — expect ~2 setup cycles (~1.5–3 min) saved per run.

## Rollback
- **Phase 1:** revert the `ci.yml` commit; the 9-job shape returns. Safe — no
  external references to job names exist until Phase 2 lands.
- **Phase 2:** re-PUT the ruleset without the `required_status_checks` rule
  (the exact 3-rule payload captured in this doc's snapshot) to restore the
  advisory-CI state.

## Risks / notes
- **Ordering:** land Phase 1 (job rename) **before** Phase 2 (require the new
  names). If Phase 2 lands first, the required contexts never report and every
  PR hangs "Expected — waiting for status." Same-PR is fine **only** if the PR
  is merged with admin override once; cleaner to split into two PRs.
- **Lost granularity:** a `lint-and-test` failure no longer says "lint vs test"
  in the checks list — the maintainer reads the step logs. Acceptable; both
  were already one `pnpm guard:pre-push` locally.
- **`required_status_checks` context matching** is by exact string. A typo in
  the rule (e.g. `frontend-check` vs `frontend-checks`) silently waits forever.
  Phase-2 verification step 3 catches this.
- Solo-maintainer (`owner.type: User`): rulesets apply to the owner too unless
  bypass is configured; the implementing PR's own CI must pass to self-merge
  after Phase 2.
