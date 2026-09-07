# Documentation

Use these guides for current behavior. Plans and ADRs preserve design history;
proposed work is not automatically implemented.

| Guide | Purpose |
|---|---|
| [Getting started](./GETTING_STARTED.md) | Install, choose a model, generate an artifact |
| [Project status](./PROJECT_STATUS.md) | Implemented capabilities and planned work |
| [Architecture](./ARCHITECTURE.md) | Code ownership, contracts, and data boundaries |
| [Feature review](./FEATURE_REVIEW.md) | Important limitations |
| [Contributing](../CONTRIBUTING.md) | Pick and submit a useful change |
| [CI/CD](./CI_CD.md) | Local checks, required jobs, delivery |
| [Releasing](./RELEASING.md) | Validate a tag and review draft installers |
| [Agent workflow](./AGENT_WORKFLOW.md) | Carry a scoped task through a PR |
| [Branch protection](../BRANCH_PROTECTION.md) | Repository merge rules |

## AI-first work

[Roadmap](../plan/ROADMAP.md) →
[staged-review contract](../plan/versions/v2/AI_FIRST_REVIEW.md) →
[checkout fixture](../evals/README.md).

The contract and fixture are available. The review runtime, desktop UI, and
evaluation harness remain delivery work. CLI and MCP are later integrations.

## Reference and history

- [Versioned plans](../plan/versions/README.md): implementation status varies by
  feature; folder names do not imply published releases.
- [Backend ADRs](../apps/desktop/src-tauri/docs/adr/README.md): accepted decisions
  and migration proposals.
- [Engineering rules](../rules/rules.md): conventions and original design context.
- [Changelog](../CHANGELOG.md): change history.
- [Shared contracts](../packages/shared/README.md),
  [ESLint](../packages/eslint-config/README.md),
  [TypeScript](../packages/tsconfig/README.md).

Update current behavior and design status in the same PR. Preserve historical
decisions and link to replacements when superseded.
