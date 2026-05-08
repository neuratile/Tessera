# Contributing

Short rules. Read once. The full agent / contributor workflow lives
in [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md) — read that
before opening a PR. Branch-protection setup lives in
[`BRANCH_PROTECTION.md`](./BRANCH_PROTECTION.md) (admin only).

## One-time setup

```bash
git clone https://github.com/Rajveerx11/Tessera.git tessera
cd tessera
corepack enable
corepack pnpm install      # also installs Husky hooks via `prepare`
```

After this, `git commit` and `git push` automatically run the local
guards — no manual hook install needed.

## Branch / commit / PR

- Branch from `master`. Name: `feat/<short>`, `fix/<short>`,
  `chore/<short>`, etc.
- Conventional Commits. Body explains **why**, not what.
- Open a PR against `master`. CI must be green before merge. Branch
  protection blocks merging until reviews + checks pass.

## Pre-push gauntlet (runs automatically)

`git push` triggers `.husky/pre-push` → `tools/scripts/pre-push.sh`,
which mirrors the required CI checks:

1. conflict-marker scan
2. `pnpm typecheck`
3. `pnpm lint`
4. `pnpm test` (Vitest + Rust unit tests)
5. `cargo clippy` + `cargo test --lib` (only if cargo is installed)

To run it manually before pushing:

```bash
pnpm guard:pre-push
```

Failing locally is faster than failing in CI. Do not bypass with
`--no-verify` unless the user explicitly asked you to — branch
protection will reject the PR anyway.

## Merge conflicts

Master has been broken **three times** by commits that included
unresolved `<<<<<<<` / `=======` / `>>>>>>>` markers. The
`conflict-marker-check` CI job and the local pre-commit hook both
catch this; do not bypass them.

Correct flow when `git pull` (or `git rebase`) reports conflicts:

```bash
git pull --rebase origin master    # rebase, do not merge

# Conflict reported. Stop.
git status                          # lists "both modified" files

# For each unmerged file:
#  - open in editor
#  - delete every <<<<<<< / ======= / >>>>>>> line
#  - keep the resolved content
#  - save

git add <resolved-files>
git rebase --continue
pnpm guard:pre-push
git push --force-with-lease
```

Never `git commit -a` while a rebase is unresolved — that ships
markers to remote.

## Don't push directly to master

Even before branch protection is enforced server-side, treat `master`
as PR-only. Direct pushes that skip the gate are how the marker bug
keeps recurring.

## Rules

Follow [`rules/rules.md`](./rules/rules.md). Highlights:

- TypeScript strict; no `any`; Zod at every external boundary.
- Rust `#![deny(clippy::all)] + #![warn(clippy::pedantic)]`. No
  `unwrap()` / `expect()` in production paths.
- All SQL parameterized via `sqlx::bind`. No string concat.
- API keys encrypted at rest (AES-GCM). Never logged.
- LLM output is untrusted. Never feed it to `dangerouslySetInnerHTML`
  or `rehype-raw`.

## Plan docs

Multi-day work needs a plan in `/plan` first. PR description links
the plan. Reviewer reads the plan before the diff.

## See also

- [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md) — full workflow,
  hard rules, AI-agent-specific guardrails
- [`BRANCH_PROTECTION.md`](./BRANCH_PROTECTION.md) — admin runbook for
  the GitHub branch-protection settings
- [`rules/rules.md`](./rules/rules.md) — engineering rules
- `plan/` — phase plans and design docs
