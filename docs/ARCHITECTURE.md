# Architecture and data flow

React presents project/artifact state. Rust owns filesystem access,
orchestration, persistence, provider calls, and runners.

```text
React components / stores
  -> typed Tauri IPC
  -> Rust commands (validate and delegate)
  -> services (coordinate)
      -> repositories -> SQLite
      -> AST / retrieval / prompts -> selected providers
      -> runners -> opt-in local Docker sandbox
```

| Layer | Location and responsibility |
|---|---|
| Renderer | `apps/desktop/src`: UI, stores, IPC helpers, frontend tests |
| Commands | Backend `commands`: boundary validation and delegation |
| Services | Backend `services`: orchestration, budgets, cancellation |
| Repositories | Backend `repositories`: SQL and persistence |
| Providers/prompts | Backend `providers`, `prompts`: payloads and prompt logic |
| Runners | Backend `providers/runners`: language execution and shared hardened harness |
| Contracts | `packages/shared/src`: Zod validation and inferred TS types |
| Optional API | `apps/server`: Boards service |

Backend paths are under `apps/desktop/src-tauri/src`. Rust serde DTOs define
backend wire behavior; keep Zod validation aligned, including naming and optional
fields. Add malformed-input and provider/model boundary tests when contracts change.

## Invariants

Activating one LLM configuration transactionally deactivates the rest. The
renderer honors the explicit selection without arbitrary first-row fallback.
Embedding selection is independent; indexing/retrieval enforce provider and
dimension compatibility.

SQLite stores embedding BLOBs searched with cosine similarity.
[ADR-0002](../apps/desktop/src-tauri/docs/adr/0002-vec0-migration-trigger.md)
describes a future vec0 migration, not an active automatic migration.

## Data boundaries

Local Ollama generation/embeddings keep code context local. Cloud providers
receive relevant prompts/snippets. Jira export, the optional Boards API, and
configured error-reporting services can also create outbound traffic.

Sandbox execution needs explicit opt-in. The Docker harness uses no network,
a non-root user, dropped capabilities, read-only root filesystem, resource
limits, and timeout/cancellation. Preserve these boundaries and their tests.
See [ADR-0004](../apps/desktop/src-tauri/docs/adr/0004-sandbox-test-runner.md).

The planned [review contract](../plan/versions/v2/AI_FIRST_REVIEW.md) adds
read-only Git capture and immutable source/test identities. Build it within
these layers first; reusable core, CLI, and MCP follow proven desktop behavior.
