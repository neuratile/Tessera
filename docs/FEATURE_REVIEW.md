# Feature review

This is a source-based capability review, not a numerical product rating.
See [Project status](./PROJECT_STATUS.md) for current availability.

| Area | Useful today | Limit or next step |
|---|---|---|
| Generation | Structured artifacts with code context | Model claims need review; they are not execution evidence |
| Providers | Explicit active LLM and separate embeddings | Preserve payload, failure, and dimension-mismatch checks |
| Retrieval | AST chunks and persisted embeddings | BLOB cosine scans; vec0 remains deferred |
| Editor | Tabs, syntax highlighting, in-memory edits | No general source save action |
| Sandbox | JS/TS and Python, cancellation, limits, coverage | Docker required; no Go runner |
| Healing | Bounded reruns and generated-test history | Passing tests do not prove application repair |
| Flaky checks | Repeated runs and history | Sampled stability cannot prove absence of flakiness |
| Mutation | JS/TS scoring and survivor-driven improvement | Supported mutations only; score is not correctness proof |
| Export | Files, clipboard/spreadsheets, Jira Cloud | More trackers are later work |
| Testing | Unit, contracts, renderer E2E, Docker, advisory coverage/live models | E2E mocks IPC; installers still need platform smoke tests |
| Review | Contract and deterministic fixture | Capture, persistence, context, findings, UI, evidence binding, evaluation remain |

Build one trustworthy staged review before expanding integrations: capture
without changing Git, preserve exact source and line locations, distinguish
suspected/reproduced/inconclusive evidence, then measure seeded bugs and clean
controls. See the [contract](../plan/versions/v2/AI_FIRST_REVIEW.md) and
[roadmap](../plan/ROADMAP.md).
