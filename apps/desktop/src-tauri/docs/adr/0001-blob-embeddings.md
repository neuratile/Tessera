# ADR-0001: BLOB embeddings + brute-force cosine for MVP RAG

- **Status**: Accepted
- **Date**: 2026-05-02
- **Authors**: Backend / AI Pipeline (Student 2)
- **Supersedes**: none
- **Superseded by**: none (yet)

## Context

The Phase 1 schema reserves a column on `code_chunks` for the embedding
vector used by RAG retrieval. Two design axes are in play:

1. **Storage shape** — fixed-dim `VECTOR(N)` (sqlite-vec `vec0` virtual
   table) versus variable-length `BLOB` holding a packed `f32` array.
2. **Search algorithm** — sqlite-vec ANN index (HNSW-style) versus
   brute-force cosine over the candidate set.

The initial plan's first cut suggested `VECTOR(1024)`. Realities that
forced a re-evaluation:

- The provider abstraction (`rules.md` §5.2 + §12.2) covers four LLM
  providers and at least four embedding providers with different output
  dimensions — Ollama `nomic-embed-text` (768), OpenAI
  `text-embedding-3-small` (1536) and `-large` (3072), Voyage AI (1024).
  A fixed-dim column locks the schema to one provider.
- sqlite-vec is a SQLite loadable extension. With statically-linked sqlx
  0.8 it must be registered as a sqlite3 auto-extension before any
  connection opens. The exact API (`libsqlite3-sys` exposure, FFI symbol
  registration, sqlx feature compatibility) was not verified during
  Phase 1 and would have blocked progress on the rest of the foundation.
- MVP scale is small. A typical analyzed project produces a few hundred
  to ~10K chunks. Brute-force cosine over 10K × 768-dim `f32` vectors
  takes ~5–20 ms in Rust on a modern CPU — well below human-perception
  threshold and inside the LLM-inference budget.

## Decision

**Store embeddings as variable-length `BLOB` and search via brute-force
cosine, scoped to a single `(project_id, embedding_provider,
embedding_dim)` tuple. Cap MVP at 10 000 chunks per project and refuse
oversize projects with `AppError::LimitExceeded`. Migrate to
sqlite-vec `vec0` (Phase 3+) when chunk counts cross the cap.**

Concretely:

- `code_chunks.embedding` — `BLOB`, packed `f32` little-endian.
- `code_chunks.embedding_dim` — `INTEGER`, dimension of the vector.
- `code_chunks.embedding_provider` — `TEXT`, e.g. `ollama-nomic-embed-text`.
- `code_chunks.embedding_model` — `TEXT`, full model identifier.
- `CHECK` constraint: all four columns are NULL together (chunk not yet
  indexed) or all four are populated (chunk fully attributed). No
  partial-write states.
- Composite index `idx_code_chunks_search_scope` on `(project_id,
  embedding_provider, embedding_dim)` so the candidate set is narrowed
  by the SQLite query planner before any cosine math runs.
- Search WHERE clause **must** filter by all three of project_id,
  embedding_provider, embedding_dim — never compare across dimensions.

## Consequences

### Positive

- Provider-agnostic by construction. Switching from Ollama to OpenAI
  embeddings requires zero schema work.
- One fewer compile-time dep (`libsqlite3-sys` not needed during Phase 1).
- No FFI auto-extension wiring during Phase 1 — Phase 3 owns that risk.
- Migration path forward is monotonic — adding a `code_chunks_vec`
  virtual table in Phase 3 leaves the existing column untouched and
  backfills from the BLOB column.

### Negative

- Brute-force cosine is `O(n)` per query. At 10K chunks × 768 dim it is
  fine; at 1 M chunks per project it is not. Hard cap at 10K avoids
  surprise regressions.
- No ANN index until Phase 3 / ADR-0002. Cold queries pay full O(n).
- `embedding_dim` filter must be remembered at every call site.
  Mitigation: the repository layer (`chunk_repo`) is the only place that
  reads embeddings — services never touch the BLOB directly. Filter
  enforced once, used everywhere.

### Mitigations

- Phase 3 chunk_repo benchmark (`tests/bench_cosine_10k.rs`) fails the
  test suite if cosine over 10 000 768-dim chunks exceeds 100 ms on
  reference hardware. Early warning that ANN migration is overdue.
- `// PERF: brute-force cosine, migrate to sqlite-vec vec0 above 50 000
  chunks. See ADR-0001 + ADR-0002.` comment lives at the cosine
  function in `chunk_repo.rs` (added in Phase 3).
- ADR-0002 ("sqlite-vec vec0 migration trigger") will land alongside
  Phase 3 to lock the cap and document the migration shape.

## Alternatives considered

1. **Fixed-dim `VECTOR(1024)` column**: rejected. Locks schema to one
   provider, breaks day-1 portability, no upside at MVP scale.
2. **sqlite-vec `vec0` from day one**: deferred. Worth doing eventually,
   too much FFI-wiring risk for Phase 1 foundation.
3. **External vector DB (Qdrant, Chroma)**: rejected. Conflicts with
   `rules.md` §1.8 (local-first) and the embedded `sqlite-vec` stack.

## References

- `rules.md` §1.8 (local-first), §2.3 (migrations), §5.2 (provider
  abstraction), §12.3 (RAG / embeddings)
- `apps/desktop/src-tauri/migrations/0001_init.sql` — schema
- `apps/desktop/src-tauri/src/db/mod.rs` — pool init, deferral note
