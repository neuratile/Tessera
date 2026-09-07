# Contributing to Tessera

Start with [Getting started](./docs/GETTING_STARTED.md) and
[Project status](./docs/PROJECT_STATUS.md). The priority is trustworthy staged
review; the runtime is still being built.

## Pick a scoped task

Read [roadmap #104](https://github.com/neuratile/Tessera/issues/104) and the
[delivery order](./plan/ROADMAP.md). Check dependencies before starting.
Focused docs, fixtures, and boundary tests are useful entry points.

The [checkout fixture](./evals/README.md) needs only Node and Git:

```bash
node --test evals/fixtures/checkout/fixture.test.mjs
```

Keep baseline and clean controls passing while the seeded bug fails as expected.
Fixture success is not model accuracy until a harness evaluates real reviews.

## Submit a change

1. Fetch origin/master and branch for one task. Use separate worktrees for parallel work.
2. Describe the trigger, expected behavior, and acceptance check.
3. Preserve backend layering and the explicit provider choice; read
   [Architecture](./docs/ARCHITECTURE.md) and [AGENTS](./AGENTS.md).
4. Add meaningful tests for behavior/contracts and update affected documentation.
5. Run `pnpm guard:pre-push` before pushing; the hook repeats it automatically.
6. Open a PR, inspect CI and review feedback, and fix findings on the same branch.

Do not commit secrets, installers, unrelated cleanup, or conflict markers.
Use Conventional Commit subjects such as `fix: preserve review source locations`.

Use the [PR template](./.github/pull_request_template.md). Explain the problem,
resulting behavior, checks, and unverified limits. Include screenshots for UI
changes; call out provider, persistence, sandbox, or configuration changes.
Keep planned behavior explicitly labeled.

[CI/CD](./docs/CI_CD.md) lists required checks and local/CI differences.
Squash merge through a PR. Agents leave merging and release publication to the
user unless explicitly authorized. See [Agent workflow](./docs/AGENT_WORKFLOW.md).
