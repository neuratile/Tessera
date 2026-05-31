# Branch protection — admin runbook

Branch protection rules can only be flipped by a repo admin in the
GitHub UI. Apply these settings exactly once after this PR lands; the
rest of the workflow (hooks, CI, CODEOWNERS, auto-merge) is already
wired up in the repo.

> **Why this exists.** Master has been broken three times by direct
> merges and conflict-marker commits. This document is the canonical
> "how master stays green" reference for admins.

---

## 1. Apply branch protection on `master`

GitHub UI path: **Settings → Branches → Branch protection rules → Add rule**.

- **Branch name pattern**: `master`

Tick every box below:

### Required pull-request review

- [x] Require a pull request before merging
- [x] Require approvals — **set to 1** (raise to 2 when the team
      crosses 5 people)
- [x] Dismiss stale pull request approvals when new commits are pushed
- [x] Require review from Code Owners
- [x] Require approval of the most recent reviewable push

### Required status checks

- [x] Require status checks to pass before merging
- [x] Require branches to be up to date before merging

Then add **all** of these checks as required (type the names exactly —
GitHub will show them in the dropdown after the first CI run):

- `conflict-marker-check`
- `lint`
- `typecheck`
- `unit-test`
- `integration-test (ubuntu)`
- `release-build`

If a check name does not appear, run CI once on a throwaway PR so
GitHub indexes the job names, then refresh this page.

### History + conversations

- [x] Require conversation resolution before merging
- [x] Require linear history
- [x] Require deployments to succeed before merging — leave **off**
      unless we wire a staging deploy

### Pushes

- [x] Restrict who can push to matching branches — leave the allow-list
      **empty** so no human bypass is possible
- [x] Do not allow bypassing the above settings (admins included)
- [ ] Allow force pushes — **OFF**
- [ ] Allow deletions — **OFF**

Save the rule.

---

## 2. Repository settings

GitHub UI path: **Settings → General**.

Under **Pull Requests**:

- [x] Allow squash merging — set commit message to **"Pull request
      title and description"**
- [ ] Allow merge commits — **OFF** (linear history)
- [ ] Allow rebase merging — **OFF** (squash is the only way in)
- [x] Always suggest updating pull request branches
- [x] Automatically delete head branches

Under **Actions → General**:

- [x] Allow GitHub Actions to create and approve pull requests
      (required for the `auto-merge` workflow's `gh pr merge --auto`
      to work on its own PRs — keep off if not using Dependabot)

---

## 3. Secrets / variables

No new secrets are required for the gating itself. The release
workflow already uses `GITHUB_TOKEN` and `TAURI_*` signing secrets;
nothing in this rollout touches them.

---

## 4. Verify

Open a one-line throwaway PR and confirm:

- The PR template auto-fills.
- A reviewer is auto-requested via CODEOWNERS.
- The "Merge" button is greyed out until reviews + checks are green.
- Direct `git push origin master` from the CLI is rejected with
  `protected branch hook declined`.

If any of those four signals are missing, re-check the rule above.

---

## 5. Local hooks

Per-developer hook setup is automatic on `pnpm install` and is a contributor
concern, not an admin one — see [`CONTRIBUTING.md`](./CONTRIBUTING.md). Branch
protection makes a `--no-verify` bypass useless anyway: the PR is still gated by CI.
