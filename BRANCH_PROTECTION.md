# Branch protection

The `Protect master` repository ruleset (ID `17259460`) protects master.
Inspect live settings before an authorized administrative change:

```bash
gh api repos/neuratile/Tessera/rulesets/17259460
gh api repos/neuratile/Tessera --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, allow_auto_merge}'
```

## Current policy

| Rule | Effect |
|---|---|
| `deletion` | Prevents deletion of master |
| `non_fast_forward` | Prevents force pushes; this alone does not prohibit merge commits |
| `pull_request` | Requires PRs; squash-only merge method; zero required approvals |
| `required_status_checks` | Six required checks; up-to-date/strict policy off |

Keep these names exactly aligned with the CI jobs:

- `conflict-marker-check`
- `frontend-checks`
- `lint-and-test`
- `server-check`
- `e2e-test`
- `sandbox-runner-test`

Coverage and `integration-test (ubuntu)` are advisory. See
[CI/CD](./docs/CI_CD.md) for each job's scope. Review is encouraged even though
the configured approval count is zero. Squash-only policy preserves linear
history for PR merges.

## Change and verify

Ruleset changes require maintainer authorization. Fetch the live ruleset,
review a complete proposed payload, and preserve all unrelated rules; the API
replaces the rules array. Required status context names must match workflow
job names or merges can wait indefinitely.

Verify read-only through the API, the Settings → Rules → Rulesets UI, and checks
on an ordinary PR. Do not test protection by pushing directly to master or
deliberately committing failing tests.

Auto-merge is opt-in through the existing `auto-merge` label and respects current
rules. It does not require approvals/up-to-date status beyond what the ruleset
actually configures. Do not enable it on behalf of a user without authorization.

## Hooks and releases

`pnpm install` configures Husky hooks. Run `pnpm guard:pre-push` before pushing;
CI independently enforces its required checks. Local checks are not full CI
parity and must not be bypassed to hide failures.

Release jobs validate tags and run reusable CI before creating draft installers.
The workflow uses `GITHUB_TOKEN` for release assets and does not configure
platform signing secrets. See [Releasing](./docs/RELEASING.md).
