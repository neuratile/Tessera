# Tessera

Local-first AI testing for your codebase.

[![CI](https://github.com/neuratile/Tessera/actions/workflows/ci.yml/badge.svg)](https://github.com/neuratile/Tessera/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE.md)

Tessera is a Tauri desktop app that reads a local project, retrieves relevant
code, and uses your selected model to generate test plans, test cases, and bug
reports. Run generated JS/TS or Python tests in an opt-in Docker sandbox,
inspect coverage, and export results.

**Next: Review my changes.** We are building a staged Git review with code-linked
findings and evidence tied to exact source/test versions. The
[contract](./plan/versions/v2/AI_FIRST_REVIEW.md) and
[checkout fixture](./evals/README.md) are available. The review engine and UI
are **not implemented yet**. Follow [roadmap #104](https://github.com/neuratile/Tessera/issues/104).

## What works today

- Import a local project and build context using AST analysis and embeddings.
- Generate Context, Test Plan, Test Cases, Defect Report, and Bug Report artifacts.
- Select one active LLM: Ollama, OpenAI, OpenRouter, Anthropic, or Gemini.
  Select embeddings independently: Ollama, OpenAI, Gemini, or Hugging Face.
- Execute supported tests with explicit Docker sandbox opt-in; inspect coverage.
- Repeat runs for flaky checks, refine generated tests with a bounded healing
  loop, and score/improve JS/TS tests against mutations.
- Export Markdown, JSON, spreadsheet/tabular data, or push artifacts to Jira Cloud.

A passing regenerated test does not establish that application code was repaired.
The planned review workflow will preserve that distinction in its evidence.

## Start locally

Install Git, pnpm 10.9.0, stable Rust, and the native
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.
CI uses Node 20; the optional TypeScript bootstrap helper needs Node 22.6+.
Docker is required only for sandbox execution or containerized services.

```bash
git clone https://github.com/neuratile/Tessera.git tessera
cd tessera
corepack enable
pnpm install --frozen-lockfile
cp apps/desktop/.env.example apps/desktop/.env
pnpm --filter @testing-ide/desktop dev
```

Follow [Getting started](./docs/GETTING_STARTED.md) to configure Ollama and
generate your first artifact. Models are downloaded separately.

**Generation stays local when you select local providers.** Cloud LLM and
embedding selections send relevant prompts/code context to those providers.
Jira and the optional Boards server also use network services. Sandbox containers
run without network access. See [Architecture](./docs/ARCHITECTURE.md).

## Contribute to the AI-first workflow

Read [CONTRIBUTING](./CONTRIBUTING.md) and the [delivery order](./plan/ROADMAP.md).
Pick one scoped issue and include a reproducible acceptance check.

The smallest starting point needs only Node and Git:

```bash
node --test evals/fixtures/checkout/fixture.test.mjs
```

This checks a seeded checkout bug and a clean control without an LLM, Docker,
or the desktop app. It is a fixture check, not a working review or model accuracy
score. Git capture, contracts, context, and findings come next; core extraction,
CLI, and MCP follow a proven desktop workflow.

## Repository map

| Location | Purpose |
|---|---|
| `apps/desktop/src` | React renderer, stores, frontend tests |
| `apps/desktop/src-tauri/src` | Rust commands, services, repositories, providers, runners |
| `apps/server` | Optional Boards API, checked separately in CI |
| `packages/shared` | Zod validation and TypeScript contracts |
| `packages/eslint-config`, `packages/tsconfig` | Shared tooling presets |
| `evals` | Deterministic fixtures; review evaluation harness planned |
| `docs`, `plan` | Current guides and feature design records |
| `tools/scripts`, `.github/workflows` | Local checks and CI/CD |

## Documentation and delivery

- [Documentation index](./docs/README.md): setup, architecture, testing, and releases.
- [Project status](./docs/PROJECT_STATUS.md): implemented features and known limits.
- [Roadmap](./plan/ROADMAP.md): staged review first, integrations later.
- [CI/CD](./docs/CI_CD.md): local checks and six required PR jobs.
- [Release guide](./docs/RELEASING.md): checked tags produce draft installers.

Release automation targets Windows, macOS, and Linux. A draft build is not a
published release or proof of signing. Check the
[Releases page](https://github.com/neuratile/Tessera/releases) for public assets.

[MIT licensed](./LICENSE.md).
