# Agent workflow — how AI agents (and humans) ship changes safely

> **Audience.** Any AI coding agent (Claude Code, Cursor, Copilot
> Workspace, etc.) and any human contributor to this repo. Read this
> *before* opening an editor. Read it again if you have not pushed in
> the last week.

This file is the single source of truth for the change-management
process. It is paired with [`BRANCH_PROTECTION.md`](../BRANCH_PROTECTION.md)
(admin-only UI steps) and [`CONTRIBUTING.md`](../CONTRIBUTING.md)
(human-onboarding pointer).

If anything in this file conflicts with `rules/rules.md`,
**`rules/rules.md` wins** for code-style and architectural decisions.
This file only governs *how* a change moves from local edit to
master.

---

## 1. The core invariant

> **Master is always green and always linear.**

That means:

- Master never carries unresolved conflict markers.
- Master never has a red CI run as its `HEAD`.
- Master has no merge commits — every PR is squashed.
- No one pushes directly to master, including admins, including agents.

Every guard described below exists to defend this invariant.

---

## 2. Required setup (one-time, per machine)

```bash
git clone https://github.com/neuratile/Tessera.git tessera
cd tessera
corepack enable
corepack pnpm install
```

`pnpm install` runs the `prepare` script, which configures Git to use Husky hooks. From this point forward:

- `git commit` triggers `.husky/pre-commit` (instant guard).
- `git push` triggers `.husky/pre-push` (local baseline checks).

There is no manual hook install. If `git push` does not seem to run
the gauntlet, run `pnpm prepare` again.

Recommended additional tooling:

- **Rust toolchain** (`rustup`) — required to run the local clippy
  step; without it, `tools/scripts/pre-push.sh` skips Rust gates.
- **GitHub CLI** (`gh`) — required for `gh pr create` / `gh pr view`.

---

## 3. The change loop (every PR, every time)

### 3.1 Branch from up-to-date master

```bash
git checkout master
git pull --ff-only
git checkout -b <type>/<short-slug>
```

`<type>` is one of `feat`, `fix`, `refactor`, `perf`, `docs`, `chore`,
`test`. `<short-slug>` is a kebab-case description, ≤ 40 chars.

Examples: `feat/streaming-preview`, `fix/ollama-404-hint`,
`chore/upgrade-tauri-2.6`.

### 3.2 Make the change in small commits

Conventional Commits required (`feat:`, `fix:`, `chore:`, etc.). See
the existing `git log` for tone. Subject ≤ 72 chars; the body
explains *why*, not *what*.

Each `git commit` runs the pre-commit hook:

- conflict-marker scan
- large-file guard (> 5 MB)

If the hook rejects, fix the underlying issue rather than bypassing
with `--no-verify`. CI independently enforces its required checks; a local-only guard may not be duplicated there.

### 3.3 Run the local gauntlet before pushing

```bash
pnpm guard:pre-push
```

That script runs:

1. Conflict-marker scan.
2. Release tooling tests and deterministic checkout fixtures.
3. Workspace TypeScript checks.
4. Workspace ESLint checks.
5. Shared and frontend unit tests.
6. Desktop Rust Clippy and library tests when Cargo is installed.

This is a local baseline. Server, production renderer build, Playwright, Docker,
coverage, and live Ollama checks remain separate CI coverage. See [CI/CD](./CI_CD.md).

`git push` runs the same script automatically via the pre-push hook.
Calling it manually first is just a faster feedback loop.

If any stage fails, the script tells you which one — fix it locally
and re-run.

### 3.4 Open the PR

```bash
git push -u origin <branch>
gh pr create --fill
```

`gh pr create --fill` populates the title and body from the latest
commit and the `pull_request_template.md`. Tick the relevant boxes
in the template — do **not** delete the template.

### 3.5 Wait for CI + reviewer

CI runs these jobs. The six **required** ones are enforced by the `master`
ruleset — the merge button stays greyed out until all six are green:

| Job                          | Required? | What it asserts                                                  |
|-----------------------------|:---------:|-----------------------------------------------------------------|
| `conflict-marker-check`     | ✅        | Marker scan, release tooling tests, and checkout fixtures                   |
| `lint-and-test`             | ✅        | ESLint + clippy clean, then Vitest + Rust unit tests pass       |
| `frontend-checks`           | ✅        | TypeScript clean, then the production Vite build succeeds        |
| `server-check`              | ✅        | `apps/server` clippy + tests pass                               |
| `e2e-test`                  | ✅        | Playwright renderer suite passes (mocked Tauri IPC)             |
| `sandbox-runner-test`       | ✅        | Docker sandbox builds + the gated runner test passes            |
| `integration-test (ubuntu)` | advisory  | Live Ollama suite — `continue-on-error`, never blocks merge      |

> `lint-and-test` merges the former `lint` + `unit-test` jobs, and
> `frontend-checks` merges the former `typecheck` + `build-check`, to avoid
> re-paying an identical toolchain setup twice (see
> [`../plan/versions/v1/CI_JOB_CONSOLIDATION.md`](../plan/versions/v1/CI_JOB_CONSOLIDATION.md)). The
> cross-platform `tauri build` runs in `release.yml` on tag pushes, not on PRs.

When a valid matching `CODEOWNERS` rule exists, GitHub requests its owner. Address every comment in the PR; do not push fixes as new
branches.

### 3.6 Merge

Use **Squash and merge** only. The merge button is gated by the `master`
ruleset — it will be greyed out until:

- All six required checks green (see the table in 3.5)
- Linear history preserved (squash-only; no merge commits)

The ruleset currently requires **0 approving reviews** and does **not** force
the branch to be up to date before merge (`strict` is off), so a review is
encouraged but not a hard gate. Raise `required_approving_review_count` and flip
on the strict policy in the ruleset when the team grows — see
[`../BRANCH_PROTECTION.md`](../BRANCH_PROTECTION.md).

If you choose to bring the branch up to date with master first, do:

```bash
git fetch origin
git rebase origin/master
git push --force-with-lease
```

Never run `git push --force` without `--with-lease`. Never force-push
to master.

### 3.7 Auto-merge (optional)

Add the `auto-merge` label to the PR. The workflow at
`.github/workflows/auto-merge.yml` flips GitHub's native auto-merge
flag, so the PR squash-merges automatically the moment all gates go
green. Auto-merge does **not** bypass any required check; it only
removes the manual click.

---

## 4. Hard rules (these will fail CI / branch protection)

| Rule                                                       | Defended by                          |
|------------------------------------------------------------|--------------------------------------|
| No direct push to `master`                                 | branch protection                    |
| No merge commits on `master`                               | squash-only PR merge policy |
| No conflict markers anywhere in tracked files              | pre-commit + pre-push + CI job       |
| No files larger than 5 MB committed                        | pre-commit                           |
| No TypeScript errors                                       | pre-push + CI                        |
| No ESLint errors                                           | pre-push + CI                        |
| No failing shared/frontend unit tests locally; no failing Rust unit tests in CI | pre-push + CI          |
| No `clippy::pedantic` warnings (Rust)                      | CI (`-D warnings`)                   |
| No `.env` / secrets / API keys committed                   | reviewer + manual scan               |
| No new IPC command without a Zod schema in `packages/shared/` | reviewer (rules.md §12.3.1)         |
| No mutating action without explicit user confirmation      | rules.md (security policy)           |

---

## 5. Hard rules specific to AI agents

These exist because AI agents have a habit of "fixing" things in ways
that look helpful but break the invariants above. If you are an
agent, follow these repository guardrails within the user-authorized task.

1. **Never push directly to `master`.** Always work on a feature branch
   and open a PR. If the user types "push to master", interpret that
   as "open a PR targeting master and request review".
2. **Never bypass hooks with `--no-verify` unless the user explicitly
   types those words in the same message.** Even then, surface the
   underlying error first and ask for confirmation.
3. **Never delete or weaken a guard** — pre-commit hook, pre-push
   hook, CI job, branch protection rule, CODEOWNERS entry — without
   an explicit user instruction *and* a corresponding update to this
   file.
4. **Never commit a merge commit.** When the branch is behind master,
   rebase, do not merge. `git pull --rebase` over `git pull`.
5. **Resolve conflict markers immediately and verify the resolution.**
   Run `git grep -nE '^(<{7}|>{7}|={7})( |$)'` before every commit
   that touches a file involved in a merge or rebase.
6. **Never disable a failing test.** If a test fails, either fix the
   regression that caused it, or escalate to the user. `it.skip` /
   `#[ignore]` for previously-passing tests is grounds for revert.
7. **Run the full pre-push gauntlet locally** (`pnpm guard:pre-push`)
   before any `git push`. If it fails, fix and re-run. Do not push
   "to see what CI says".
8. **When a CI failure says "X is missing"**, read the actual error
   above the summary line. Pasting the failure to the user without
   reading it (a common agent failure mode) wastes everyone's time.
9. **Stop and ask** when:
   - the user's instruction would weaken a guard;
   - the change crosses three or more layers (commands → services →
     repositories → providers → renderer);
   - you cannot match an error to its root cause within two
     investigation steps.
10. **Update this file** when the workflow changes. The file is the
    contract; if a new gate goes in, document it here in the same PR.

---

## 6. Common failure modes (and the right response)

### "Pre-push hook fails: TypeScript errors"

Run `pnpm typecheck` directly — the output is identical but easier to
read. Fix the type errors. Do not delete `// @ts-expect-error` to
silence them.

### "Pre-push hook fails: ESLint errors"

Run `pnpm lint`. If the rule is genuinely too strict for a one-off
case, add a per-line `// eslint-disable-next-line <rule>` with a
comment explaining why — never blanket-disable a rule for a file or
package without buy-in.

### "CI fails on `conflict-marker-check`"

You committed a half-resolved merge. Run:

```bash
git grep -nE '^(<{7}|>{7}|={7})( |$)' -- ':(exclude)pnpm-lock.yaml'
```

Resolve every marker, commit, push.

### "CI fails on `integration-test (ubuntu)`: ConnectionFailed to Ollama"

The runner's chat / embedding model got evicted between tests. Verify
the warmup step still runs before the suite (see
`.github/workflows/ci.yml`). If the issue is intermittent, re-run the
job once; if it repeats, raise `OLLAMA_KEEP_ALIVE` or split the
suite.

### "Updating a branch that is behind master"

```bash
git fetch origin
git rebase origin/master
# resolve conflicts if any
git push --force-with-lease
```

### "Auto-merge did not fire after the PR went green"

Check the label, workflow logs, token permissions, repository auto-merge setting,
and required checks. Enable auto-merge only when the user has authorized it.

---

## 7. Where to look when something is wrong

| Symptom                                        | Look at                                    |
|------------------------------------------------|--------------------------------------------|
| Push refused locally                           | `tools/scripts/pre-push.sh`                |
| Commit refused locally                         | `.husky/pre-commit`                        |
| CI failed on a check name                      | `.github/workflows/ci.yml`                 |
| Reviewer not auto-requested                    | `.github/CODEOWNERS`                       |
| Merge button greyed out                        | `BRANCH_PROTECTION.md` §1                  |
| Auto-merge label exists but nothing happens    | `.github/workflows/auto-merge.yml`         |
| Engineering / architecture decision            | `rules/rules.md`                           |
| Project context / architecture overview        | `CLAUDE.md`                                |
| Setup instructions                             | `README.md`                                |
