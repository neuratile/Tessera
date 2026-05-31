# ADR-0002: sqlite-vec vec0 migration trigger and rollout plan

- **Status**: Accepted
- **Date**: 2026-05-03
- **Authors**: Backend / AI Pipeline (Student 2)
- **Supersedes**: none
- **Superseded by**: none

## Context

ADR-0001 committed Phase 1 to storing embeddings as packed `f32` BLOBs
and searching them with brute-force cosine. The decision capped MVP
scale at 10 000 chunks per project and pinned a 100 ms latency target
under that load. That ceiling is intentional — it bought us a clean
Phase 1 foundation without fighting `sqlite-vec`'s static-link FFI
auto-extension wiring.

Phase 3 lands the chunk repository and exercises the BLOB path for the
first time. We now need to commit, in writing, to **when** the BLOB
strategy gets retired in favor of `sqlite-vec`'s `vec0` virtual table,
**how** the migration runs, and **what** invalidates the trigger.

The data-shape inputs:

- Provider matrix (5 LLM providers × N embedding providers, see
  ADR-0003) means embedding `dim` is provider/model dependent
  (768 for `nomic-embed-text`, 1024 for Voyage `voyage-code-3`,
  1536 / 3072 for OpenAI `text-embedding-3-small` / `-large`).
- Per ADR-0001, `code_chunks` already carries
  `embedding_dim INTEGER NOT NULL`,
  `embedding_provider TEXT NOT NULL`,
  `embedding_model TEXT NOT NULL`, and a `CHECK` constraint that
  forbids partial-write states.
- A real customer with 5 000 source files at ~3 chunks per file lands
  at 15 000 chunks — already past the 10 000 cap.
- `sqlite-vec` 0.1.x exposes `vec0` virtual tables that hold a fixed
  `(vector(N))` column. ANN search uses `MATCH` with a per-row distance
  function. Migration cost is one `INSERT INTO ... SELECT FROM` plus
  index build.

## Decision

We will keep BLOB storage as the canonical column and add `vec0`
virtual tables as **per-(provider, dim) auxiliary indexes**. The
trigger to enable a `vec0` index is the chunk-count threshold defined
below; the `code_chunks.embedding` column itself never moves.

### Trigger

Per project, when:

```
chunk_count(project_id, embedding_provider, embedding_dim) >= VEC0_TRIGGER
```

…the chunk repository creates (idempotent) a `vec0` virtual table
named `code_chunks_vec_<provider>_<dim>` and backfills it from the
matching BLOB rows. Subsequent searches for that
`(project_id, provider, dim)` tuple route through `MATCH` against the
virtual table; below the threshold, brute-force cosine still runs.

### Threshold value

`VEC0_TRIGGER = 50_000` chunks per `(project, provider, dim)` tuple.

Justification:

- Brute-force cosine at 768 dim runs ~5–20 ms per query for 10 000
  chunks on a 2024-class laptop. Linear scaling puts 50 000 chunks at
  ~25–100 ms — at the edge of the 100 ms p99 target ADR-0001 set.
- Below 50 000, the ANN index's setup cost (extension load,
  per-`(provider, dim)` virtual table) outweighs the query speedup.
- Above 50 000, brute-force fails the latency target and the
  `vec0` index becomes load-bearing.

### Auto-extension wiring

`sqlite-vec` ships as a SQLite loadable extension. With statically
linked `sqlx` 0.8, it must be registered as an `auto_extension`
*before any connection opens* — i.e. inside `db::init_pool` before
the first `pool.connect_with(...)` call.

Phase 3 verifies the API surface empirically:

1. `db::init_pool` calls
   `sqlite_vec::sqlite3_vec_init` via `libsqlite3-sys::auto_extension`
   immediately after the `SqliteConnectOptions` are built and before
   the pool's first connection. This must be unconditional — registering
   twice is a no-op, but registering after the first connection is a
   silent failure.
2. The first migration (`0002_vec0_extension.sql`) runs `SELECT
   vec_version();` as a smoke check inside the migration; missing
   extension registration aborts the migration with a typed
   `MigrateError`, which we surface as `AppError::Migration`.
3. Phase 3 chunk-repo tests cover both code paths (extension loaded
   and extension missing) so a regression in auto-extension wiring is
   caught at `cargo test`.

### Migration shape

When the trigger fires for a tuple, the chunk repository runs:

```sql
-- One-time per (provider, dim).
CREATE VIRTUAL TABLE IF NOT EXISTS code_chunks_vec_<provider>_<dim>
USING vec0 (
    chunk_id TEXT PRIMARY KEY,
    embedding FLOAT[<dim>]
);

-- Backfill from BLOB column.
INSERT OR REPLACE INTO code_chunks_vec_<provider>_<dim> (chunk_id, embedding)
SELECT id, embedding
FROM code_chunks
WHERE project_id = ?
  AND embedding_provider = ?
  AND embedding_dim = ?;
```

The repository writes to **both** the BLOB column and the `vec0` index
on every subsequent insert (`INSERT OR REPLACE INTO code_chunks_vec_*`)
so the two never drift. Reads pick the index path when present, fall
back to brute-force otherwise.

### Rollback

If the `vec0` index produces incorrect results or the auto-extension
wiring regresses, the repository drops the virtual table and resumes
brute-force cosine. The BLOB column is canonical — no data is lost.

### Out of scope

- Cross-project search (search "all chunks across all my projects")
  is not supported in Phase 3. The trigger and index scope to one
  project at a time.
- `vec0` quantization tuning (`int8` vs `float`) — Phase 3 ships
  `float` only; quantization gets its own ADR if and when measured
  recall justifies the loss.
- HNSW vs flat-list `vec0` configuration — `sqlite-vec` 0.1 only
  supports flat-list; HNSW arrives in 0.2+ and will get a fresh ADR.

## Consequences

### Positive

- Brute-force keeps working below 50 000 chunks — most users never
  hit the trigger and pay zero extension-load cost.
- BLOB column stays canonical; no data migration ever discards rows.
- Search routing decision is local (one COUNT query) — does not need
  global state.
- `vec0` index added per `(provider, dim)` tuple matches ADR-0001's
  filter contract: searches never cross provider/dim boundaries, so
  the index doesn't need to either.

### Negative

- Per-tuple virtual tables can proliferate if a user mixes many
  providers (e.g. trying Ollama then OpenAI then Voyage on the same
  project). Mitigation: tuple count is bounded by the provider matrix
  (≤ 6 today). Acceptable.
- Index build cost on the trigger crossing — 50 000 rows × 768 dim
  ≈ 150 MB of BLOB scanned + indexed, expect ~2–10 s on commodity
  hardware. Phase 3 chunk-repo runs this off the request thread (a
  background task) so the user-visible search latency does not spike.

### Risks / Mitigations

- **Auto-extension silent failure** — registering after a connection
  opens is a no-op with no error. Mitigation: extension registration
  is the first thing `db::init_pool` does, before
  `SqlitePoolOptions::new()`. The `vec_version()` smoke check inside
  migration `0002` fails loudly if it didn't take.
- **BLOB / vec0 drift** — if writes hit the BLOB column but skip the
  index (e.g. a bug in chunk_repo.insert), brute-force and ANN return
  different results. Mitigation: a single repository method writes
  both columns inside one transaction. Tests cover the paired-write
  invariant on every insert.
- **`sqlite-vec` API churn** — 0.1.x is pre-stable. A breaking change
  in 0.2 could force re-migration. Mitigation: the BLOB column is the
  fallback; if 0.2 lands incompatibly we drop the virtual tables and
  ship without ANN until the new API is wired up.

## Alternatives considered

1. **Migrate at 10 000 chunks (matching ADR-0001's cap)** — rejected.
   Crossing the cap is a hard error per ADR-0001, but the actual
   latency target (100 ms) is met up to ~50 000. Triggering at 10 000
   pays the index-build cost on every upper-mid project for no
   measurable gain. Bumping the cap to 50 000 in the chunk-repo error
   path is a separate change shipped alongside this ADR.
2. **Drop the BLOB column once `vec0` is online** — rejected. The
   provider-agnostic storage contract from ADR-0001 (Ollama / OpenAI /
   Voyage all coexist) means we'd need one virtual table per
   `(provider, dim)` to fully replace the column, which `sqlite-vec`
   0.1 doesn't yet support without runtime DDL. Keeping BLOB canonical
   sidesteps all of that.
3. **External vector DB (Qdrant, Chroma)** — rejected for the same
   reasons as ADR-0001 § "Alternatives considered" (rules.md §1.8,
   local-first; no daemon dependency).
4. **HNSW via a third-party crate (`instant-distance`, etc.)** —
   rejected. Pulls a non-trivial dep tree, requires a separate
   on-disk format we'd have to keep in sync with the BLOB column,
   and `sqlite-vec` is the documented happy path.

## References

- `rules/rules.md` §1.8 (local-first), §2.3 (migrations),
  §5.2 (provider abstraction), §12.3 (RAG / embeddings),
  §13 (anti-pattern: god files; per-(provider,dim) tables keep
  the surface bounded).
- ADR-0001 — BLOB storage + brute-force cosine, sets the
  10 000-chunk cap that this ADR lifts to 50 000 in the chunk-repo
  error path.
- ADR-0003 — `LlmProvider` + `EmbeddingProvider` traits define the
  `(provider, dim)` tuples this index keys on.
- `apps/desktop/src-tauri/migrations/0001_init.sql` — current
  `code_chunks` schema with BLOB + dim/provider/model columns.
- `apps/desktop/src-tauri/src/db/mod.rs` — `init_pool` is where the
  auto-extension call lands.
