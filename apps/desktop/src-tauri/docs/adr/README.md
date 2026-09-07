# Architecture Decision Records

> **Scope**: backend (Rust + SQLite + LLM providers).
>
> Cross-app context is in [Architecture](../../../../../docs/ARCHITECTURE.md).

This directory holds the Architecture Decision Records for the backend
crate. Per `rules/rules.md` §7.3, every significant architectural choice
ships as a numbered ADR alongside the code that implements it. Removing
or superseding a decision requires a new ADR, never a rewrite.

## Index

| #     | Title                                                           | Status     | Implements |
| ----- | --------------------------------------------------------------- | ---------- | ---------- |
| 0001  | [BLOB embeddings + brute-force cosine for MVP RAG](./0001-blob-embeddings.md) | Accepted   | Phase 1 schema (`code_chunks`) |
| 0002  | [sqlite-vec vec0 migration trigger](./0002-vec0-migration-trigger.md) | Accepted   | Future migration; current search remains BLOB cosine |
| 0003  | [LlmProvider trait shape and streaming model](./0003-llm-provider-trait.md) | Accepted   | Phase 2 (`src/providers/llm/`) |
| 0004  | [Sandbox test runner](./0004-sandbox-test-runner.md) | Accepted | Hardened Docker execution |

Add new ADRs in numeric order; never reuse a number even after
supersession (mark the old one `Superseded by: NNNN` and leave the file
in place for the audit trail).

## Format

Every ADR begins with a frontmatter block in this exact shape:

```markdown
# ADR-NNNN: <short title>

- **Status**: Proposed | Accepted | Deprecated | Superseded
- **Date**: YYYY-MM-DD
- **Authors**: <name or stream — e.g. "Backend / AI Pipeline (Student 2)">
- **Supersedes**: none | ADR-MMMM
- **Superseded by**: none | ADR-OOOO

## Context
…
## Decision
…
## Consequences
### Positive
### Negative
### Risks / Mitigations
## Alternatives considered
## References
```

Hard rules:

- Title uses the ADR-NNNN prefix as a documentation convention, not a Clippy rule.
- All five frontmatter keys are required and appear in this order.
- Status starts at `Proposed` and only graduates after team agreement
  per `rules/rules.md` §16 (≥ 2 approvals on the merging PR).
- `## Alternatives considered` is mandatory — the file documents *why*
  this option won, which means the others must be enumerated.

## Naming

`NNNN-kebab-case-title.md` where `NNNN` is zero-padded to four digits.
Example: `0042-streaming-tool-call-batching.md`.

## When to write one

Trigger an ADR when the change is irreversible without significant
rework, when it locks in a public API surface, or when it commits the
project to a specific external dependency. Examples that did or will
warrant ADRs:

- Storage shape for embeddings (ADR-0001)
- Trait shape exposed by a layer touched by every service
  (ADR-0003 — `LlmProvider`)
- Migration trigger thresholds with measurable cost (ADR-0002 planned)
- Switching a runtime (Tokio → smol), a TLS backend (rustls → native),
  or an editor (Monaco → CodeMirror) — none yet, all would need ADRs.

Do **not** write an ADR for:

- Bug fixes
- Refactors that preserve behavior and public API
- Adding a new dependency (justify per `rules.md` §11 in the PR body
  instead)
- Internal helpers / private functions

## Review process

ADRs go through the same PR flow as code changes (`docs/adr/...` path
under feature / chore branches, Conventional Commits subject
`docs(adr): add ADR-NNNN <short title>`). The PR description must
explicitly call out "this is an ADR change" so reviewers know to scrutinize
the alternatives-considered section, not just the prose.

## Implementation status

ADR headers are reviewed manually; no ADR-specific CI linter is configured.
Accepted migration plans are not proof that the migration is active.
See [current status](../../../../../docs/PROJECT_STATUS.md).
