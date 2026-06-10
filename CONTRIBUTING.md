# Contributing

Short version. The full change-management contract — hard rules, AI-agent
guardrails, failure modes — lives in [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md);
read it before opening a PR. Admin branch-protection setup is in
[`BRANCH_PROTECTION.md`](./BRANCH_PROTECTION.md).

## One-time setup

```bash
git clone https://github.com/Rajveerx11/Tessera.git tessera
cd tessera
corepack enable && corepack pnpm install   # also wires Husky hooks via `prepare`
```

After this, `git commit` and `git push` run the local guards automatically.

## The loop

- Branch from `master`: `feat/<short>`, `fix/<short>`, `chore/<short>`, …
- [Conventional Commits](https://www.conventionalcommits.org/). Body explains **why**, not what.
- Open a PR against `master`. Branch protection blocks merge until reviews + checks pass.

`git push` triggers the pre-push gauntlet (`tools/scripts/pre-push.sh`): conflict-marker
scan → `pnpm typecheck` → `pnpm lint` → shared + desktop frontend tests →
`cargo clippy` + `cargo test --lib` (if cargo is installed). CI still runs the full
workspace `pnpm test`, so Rust stays required on GitHub even when it is skipped locally.
Run it early with `pnpm guard:pre-push`. Don't bypass with `--no-verify` — branch
protection rejects the PR anyway.

## Coverage

Coverage is **informational**, not a merge gate. Run it locally to see what the
rulebook's 80% services/utilities target (rules.md §6) actually covers:

```bash
pnpm --filter @testing-ide/desktop run test:coverage   # frontend LCOV → apps/desktop/coverage/
cargo llvm-cov --manifest-path apps/desktop/src-tauri/Cargo.toml --lib --lcov --output-path apps/desktop/src-tauri/lcov.info
```

CI's non-blocking `coverage` job emits the same two LCOV reports and uploads them
as the `coverage-lcov` artifact. `cargo llvm-cov` needs the `llvm-tools-preview`
component (`rustup component add llvm-tools-preview`) and the `cargo-llvm-cov` binary.

## Merge conflicts

Rebase, don't merge (`git pull --rebase origin master`). Resolve **every**
`<<<<<<<` / `=======` / `>>>>>>>` marker before `git rebase --continue` — the
pre-commit hook and the `conflict-marker-check` CI job both block markers (master
has been broken this way before). Walkthrough: [`docs/AGENT_WORKFLOW.md`](./docs/AGENT_WORKFLOW.md) §6.

## Rules

Follow [`rules/rules.md`](./rules/rules.md). Highlights: strict TypeScript, no `any`,
Zod at every boundary · Rust `clippy::pedantic`, no `unwrap()`/`expect()` in
production · parameterized SQL only · API keys encrypted at rest, never logged ·
LLM output is untrusted — never feed it to `dangerouslySetInnerHTML`. Multi-day
work gets a plan in `/plan` first, linked from the PR.
