<!--
Thanks for opening a PR. Keep this template short — every box should
take seconds to answer. Anything that takes longer probably belongs
in the diff itself.
-->

## Summary

<!-- 1–3 sentences. What does this PR do, and why? -->

## Type of change

- [ ] feat  — user-visible new capability
- [ ] fix   — bug fix
- [ ] refactor — internal change with no behaviour delta
- [ ] perf  — performance improvement
- [ ] docs  — documentation only
- [ ] chore — build / CI / tooling
- [ ] test  — adds or fixes tests

## Linked issues / context

<!-- e.g. Closes #123, refs #456, design doc in plan/, slack thread, etc. -->

## How was this tested?

- [ ] `pnpm typecheck`
- [ ] `pnpm lint`
- [ ] `pnpm test`
- [ ] `pnpm test:eval-fixtures` (if fixtures or review behavior changed)
- [ ] `pnpm test:tooling` (if release validation or CI changed)
- [ ] `pnpm --filter @testing-ide/desktop run test:e2e` (if UI changed)
- [ ] `cargo clippy --locked --all-targets --lib -- -D warnings` (if Rust changed)
- [ ] Manual smoke in the Tauri app (steps below)

<!-- Replace this line with manual repro steps if the change is visual or workflow-level. -->

## Pre-merge checklist

- [ ] Branch is rebased on the latest `master`
- [ ] Pre-push gauntlet (`pnpm guard:pre-push`) passes locally
- [ ] No `<<<<<<<` / `=======` / `>>>>>>>` markers in the diff
- [ ] No secrets, API keys, or `.env` files committed
- [ ] New IPC commands have matching Zod schemas in `packages/shared/`
- [ ] User-facing changes have matching tests (Vitest, Rust unit, or Playwright)
- [ ] CHANGELOG / release notes updated if this affects shipped behaviour

## Screenshots / recordings (if UI)

<!-- Drag images or short clips here. Delete the section if not applicable. -->
