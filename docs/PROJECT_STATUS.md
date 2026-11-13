# Project status

Reviewed against the repository on 2026-11-13. “Implemented” means code is on
the main development line, not that an installer is publicly released or every
provider/model combination has been evaluated.

| Area | Implemented behavior |
|---|---|
| Desktop | Tauri + React explorer, Monaco scratch editing, artifacts, settings |
| Context | Tree-sitter for JS/TS, Python, and Go; indexed code retrieval |
| Generation | Context, Test Plan, Test Cases, Defect Report, Bug Report; streaming |
| LLM | Ollama, OpenAI, OpenRouter, Anthropic, Gemini; one active connection |
| Embeddings | Ollama, OpenAI, Gemini, Hugging Face; independent selection |
| Storage | SQLite; embedding BLOBs searched with cosine similarity |
| Execution | Explicitly enabled Docker runners for JS/TS and Python, with coverage |
| Quality | Flaky history, bounded test healing/history, JS/TS mutation scoring/improvement |
| Export | Markdown/JSON, spreadsheet/tabular formats, Jira Cloud artifact push |
| Optional API | Boards service in `apps/server`, separately checked in CI |
| Review foundations | Staged-review contract and fixture, plus backend-only Git capture, session/snapshot persistence, bounded context, and findings generation |

## Planned next

The [review contract](../plan/versions/v2/AI_FIRST_REVIEW.md) defines staged
index-versus-HEAD capture, bounded context, findings, immutable snapshots, and
evidence states. Four backend pieces exist, each without a command or UI:

- Capture (#106) reads pinned Git blobs without changing Git or working files.
- Persistence (#107) adds migration `0011_reviews.sql`, `review_repo`, and
  matching Zod schemas. Terminal sessions and snapshots are immutable.
- [Context](./REVIEW_CONTEXT.md) (#108) assembles budgeted excerpts from
  captured index bytes.
- [Findings](./REVIEW_FINDINGS.md) (#109) makes one call to the active model,
  redacts credentials first, and accepts only citations that fit the supplied
  excerpts. Every finding is `suspected`.

The coordinator/IPC (#110), review UI (#111), execution evidence (#112), and
evaluation harness (#114) are not implemented. Capture retains raw source
locally. Fixture success is not a model evaluation score.

Follow [roadmap #104](https://github.com/neuratile/Tessera/issues/104) and the
[delivery order](../plan/ROADMAP.md). Prove the desktop workflow before core,
CLI, and MCP extraction.

## Known limits

- Monaco edits stay in memory; there is no general save-to-source workflow.
- Go parsing does not imply a Go sandbox runner.
- vec0 is a future migration; current retrieval uses BLOB cosine scans.
- Healing generated tests can produce passing tests without repairing the app.
- Cloud model/embedding configurations send relevant content off-device.
- Existing sandbox results are not the planned immutable staged-review evidence.
- Installer builds produce drafts; verify public availability and signing per release.

See [Feature review](./FEATURE_REVIEW.md) and [CI/CD](./CI_CD.md). Test counts
and CI status belong to the current run rather than a hardcoded documentation total.
