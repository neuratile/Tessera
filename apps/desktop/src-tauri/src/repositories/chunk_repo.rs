//! Code-chunk repository — persistence + retrieval for embedded chunks.
//!
//! Per ADR-0001 + ADR-0002: chunks land in `code_chunks` with the
//! embedding stored as a packed `f32` BLOB plus
//! `(embedding_dim, embedding_provider, embedding_model)` metadata.
//! Searches brute-force cosine over the candidate set scoped to a
//! single `(project_id, embedding_provider, embedding_dim)` tuple —
//! sqlite-vec's `vec0` index lands once the per-tuple chunk count
//! crosses [`VEC0_TRIGGER_CHUNKS`].
//!
//! This module is the *only* place in the crate that reads or writes
//! the embedding column directly (rules.md §4.2 — services do not
//! touch SQL or BLOB encoding).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::services::chunking_service::{Chunk, ChunkKind};

/// Hard cap on chunk count per `(project_id, embedding_provider,
/// embedding_dim)` tuple. Crossing this returns
/// [`AppError::LimitExceeded`] — the producer must split the project
/// or wait for sqlite-vec migration (ADR-0002).
pub const MAX_CHUNKS_PER_TUPLE: u32 = 50_000;

/// Threshold at which the chunk repository starts routing searches
/// through a `vec0` virtual table instead of brute-force cosine.
/// Phase 3 ships brute-force only; the trigger constant lives here so
/// callers can introspect it for observability / migration planning.
pub const VEC0_TRIGGER_CHUNKS: u32 = 50_000;

/// Top-K cap on every vector search. Above this the result set
/// becomes noisy and the prompt context window cannot fit them all
/// (rules.md §12.3 — "top-k capped at 50").
pub const SEARCH_TOP_K_CAP: usize = 50;

/// Tuple shape returned by the cosine-search query. Aliased to keep
/// the call site readable and to satisfy clippy's `type_complexity`
/// lint.
type SearchRow = (
    String,         // id
    String,         // file_id
    String,         // chunk_type
    Option<String>, // name
    String,         // content
    i64,            // start_line
    i64,            // end_line
    i64,            // token_count
    Vec<u8>,        // embedding BLOB
);

/// One row to insert into `code_chunks`. The repository assigns the
/// primary key, timestamps, and converts the embedding into a packed
/// little-endian `f32` BLOB.
#[derive(Debug, Clone)]
pub struct ChunkInsert {
    pub project_id: String,
    pub file_id: String,
    pub chunk: Chunk,
    /// Embedding vector. Length must match `embedding_dim` field set
    /// by the producer; the repository validates and rejects
    /// mismatches with [`AppError::InvalidInput`].
    pub embedding: Vec<f32>,
    pub embedding_dim: u32,
    pub embedding_provider: String,
    pub embedding_model: String,
}

/// Search hit returned by [`search_similar`]. Includes the cosine
/// similarity score so callers can rerank or threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkHit {
    pub id: String,
    pub file_id: String,
    pub kind: ChunkKind,
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub token_count: u32,
    pub similarity: f32,
}

/// Insert a batch of chunks atomically. Returns the assigned IDs in
/// the same order the inputs were given.
///
/// # Errors
///
/// - [`AppError::LimitExceeded`] when the post-insert chunk count
///   for any `(project_id, embedding_provider, embedding_dim)` tuple
///   would exceed [`MAX_CHUNKS_PER_TUPLE`].
/// - [`AppError::InvalidInput`] when an embedding length disagrees
///   with its declared dimension.
/// - [`AppError::Database`] for any SQLx-level failure (transaction
///   begin / commit, FK violation, etc.).
pub async fn insert_batch(pool: &SqlitePool, inserts: Vec<ChunkInsert>) -> AppResult<Vec<String>> {
    if inserts.is_empty() {
        return Ok(Vec::new());
    }

    // Check tuple-cap *before* opening a transaction so the failure
    // path does not have to roll back. For each unique tuple we run
    // one COUNT() that rolls up the existing rows.
    enforce_tuple_caps(pool, &inserts).await?;

    // Validate embedding lengths.
    for insert in &inserts {
        if insert.embedding.len() != insert.embedding_dim as usize {
            return Err(AppError::InvalidInput(format!(
                "embedding length {} does not match declared dim {}",
                insert.embedding.len(),
                insert.embedding_dim
            )));
        }
    }

    let mut tx = pool.begin().await?;
    let now = Utc::now().to_rfc3339();
    let mut ids = Vec::with_capacity(inserts.len());

    for insert in inserts {
        let id = Uuid::new_v4().to_string();
        let chunk_type = chunk_kind_to_str(insert.chunk.kind);
        let blob = encode_embedding(&insert.embedding);

        sqlx::query(
            "INSERT INTO code_chunks \
             (id, project_id, file_id, chunk_type, name, content, \
              start_line, end_line, token_count, \
              embedding, embedding_dim, embedding_provider, embedding_model, \
              metadata, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', ?, ?)",
        )
        .bind(&id)
        .bind(&insert.project_id)
        .bind(&insert.file_id)
        .bind(chunk_type)
        .bind(if insert.chunk.name.is_empty() {
            None
        } else {
            Some(&insert.chunk.name)
        })
        .bind(&insert.chunk.content)
        .bind(i64::from(insert.chunk.start_line))
        .bind(i64::from(insert.chunk.end_line))
        .bind(i64::try_from(insert.chunk.token_count).map_err(|_| {
            AppError::InvalidInput(format!(
                "chunk token_count {} exceeds i64::MAX",
                insert.chunk.token_count
            ))
        })?)
        .bind(&blob)
        .bind(i64::from(insert.embedding_dim))
        .bind(&insert.embedding_provider)
        .bind(&insert.embedding_model)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        ids.push(id);
    }

    tx.commit().await?;
    Ok(ids)
}

/// Brute-force cosine search over the chunks for one project +
/// provider + dimension combination. The top-K is clamped to
/// [`SEARCH_TOP_K_CAP`] regardless of caller request.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `query_embedding.len() !=
///   embedding_dim` (cross-dim comparison is undefined).
/// - [`AppError::Database`] for SQLx-level failures.
pub async fn search_similar(
    pool: &SqlitePool,
    project_id: &str,
    embedding_provider: &str,
    embedding_dim: u32,
    query_embedding: &[f32],
    top_k: usize,
) -> AppResult<Vec<ChunkHit>> {
    if query_embedding.len() != embedding_dim as usize {
        return Err(AppError::InvalidInput(format!(
            "query embedding length {} does not match dim {}",
            query_embedding.len(),
            embedding_dim
        )));
    }
    let top_k = top_k.clamp(1, SEARCH_TOP_K_CAP);

    let rows: Vec<SearchRow> = sqlx::query_as(
        "SELECT id, file_id, chunk_type, name, content, start_line, end_line, \
             token_count, embedding \
             FROM code_chunks \
             WHERE project_id = ? \
               AND embedding_provider = ? \
               AND embedding_dim = ? \
               AND embedding IS NOT NULL",
    )
    .bind(project_id)
    .bind(embedding_provider)
    .bind(i64::from(embedding_dim))
    .fetch_all(pool)
    .await?;

    let q_norm = vector_norm(query_embedding);
    if q_norm == 0.0 {
        // A zero query vector has no defined direction; return empty
        // rather than producing NaN scores.
        return Ok(Vec::new());
    }

    let mut hits: Vec<ChunkHit> = Vec::new();
    for (id, file_id, chunk_type, name, content, start_line, end_line, token_count, blob) in rows {
        let Some(embedding) = decode_embedding(&blob, embedding_dim) else {
            continue;
        };
        let v_norm = vector_norm(&embedding);
        if v_norm == 0.0 {
            continue;
        }
        let dot = dot_product(query_embedding, &embedding);
        let similarity = dot / (q_norm * v_norm);
        let Some(kind) = str_to_chunk_kind(&chunk_type) else {
            continue;
        };
        let start_line_u32 = u32::try_from(start_line).map_err(|_| {
            AppError::Database(sqlx::Error::Decode(
                format!("chunk {id} has out-of-range start_line {start_line}").into(),
            ))
        })?;
        let end_line_u32 = u32::try_from(end_line).map_err(|_| {
            AppError::Database(sqlx::Error::Decode(
                format!("chunk {id} has out-of-range end_line {end_line}").into(),
            ))
        })?;
        let token_count_u32 = u32::try_from(token_count).map_err(|_| {
            AppError::Database(sqlx::Error::Decode(
                format!("chunk {id} has out-of-range token_count {token_count}").into(),
            ))
        })?;
        hits.push(ChunkHit {
            id,
            file_id,
            kind,
            name: name.unwrap_or_default(),
            start_line: start_line_u32,
            end_line: end_line_u32,
            content,
            token_count: token_count_u32,
            similarity,
        });
    }

    // Higher similarity first.
    hits.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(top_k);
    Ok(hits)
}

/// Count the indexed chunks for one tuple. Phase 5 will read this to
/// decide whether to enable the `vec0` index path; Phase 3 exposes it
/// so tests + observability can verify the trigger threshold.
///
/// # Errors
///
/// Returns [`AppError::Database`] on `SQLx` errors.
pub async fn count_for_tuple(
    pool: &SqlitePool,
    project_id: &str,
    embedding_provider: &str,
    embedding_dim: u32,
) -> AppResult<u32> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM code_chunks \
         WHERE project_id = ? AND embedding_provider = ? AND embedding_dim = ?",
    )
    .bind(project_id)
    .bind(embedding_provider)
    .bind(i64::from(embedding_dim))
    .fetch_one(pool)
    .await?;
    Ok(u32::try_from(row.0).unwrap_or(u32::MAX))
}

/// One distinct `(provider, model, dimension)` signature present in a
/// project's embedded chunks, with its row count. Used by
/// `embedding_config_service::index_status` to detect stale indexes
/// after the user switches embedding provider/model.
#[derive(Debug, Clone)]
pub struct EmbeddingSignature {
    pub provider: String,
    pub model: String,
    pub dimension: u32,
    pub chunk_count: u64,
}

/// Distinct embedding signatures for one project's embedded chunks.
/// Empty for never-indexed projects.
///
/// # Errors
///
/// Returns [`AppError::Database`] on `SQLx` errors.
pub async fn embedding_signatures(
    pool: &SqlitePool,
    project_id: &str,
) -> AppResult<Vec<EmbeddingSignature>> {
    let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT embedding_provider, embedding_model, embedding_dim, COUNT(*) \
         FROM code_chunks \
         WHERE project_id = ? AND embedding IS NOT NULL \
         GROUP BY embedding_provider, embedding_model, embedding_dim",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(provider, model, dimension, count)| EmbeddingSignature {
            provider,
            model,
            dimension: u32::try_from(dimension).unwrap_or(u32::MAX),
            chunk_count: u64::try_from(count).unwrap_or(0),
        })
        .collect())
}

async fn enforce_tuple_caps(pool: &SqlitePool, inserts: &[ChunkInsert]) -> AppResult<()> {
    use std::collections::HashMap;
    let mut buckets: HashMap<(String, String, u32), u32> = HashMap::new();
    for insert in inserts {
        let key = (
            insert.project_id.clone(),
            insert.embedding_provider.clone(),
            insert.embedding_dim,
        );
        *buckets.entry(key).or_insert(0) =
            buckets.get(&key).copied().unwrap_or(0).saturating_add(1);
    }
    for ((project_id, provider, dim), incoming) in buckets {
        let existing = count_for_tuple(pool, &project_id, &provider, dim).await?;
        if existing.saturating_add(incoming) > MAX_CHUNKS_PER_TUPLE {
            return Err(AppError::LimitExceeded(format!(
                "chunk count for ({project_id}, {provider}, dim={dim}) would exceed {MAX_CHUNKS_PER_TUPLE}"
            )));
        }
    }
    Ok(())
}

fn chunk_kind_to_str(kind: ChunkKind) -> &'static str {
    match kind {
        ChunkKind::Function => "function",
        ChunkKind::Method => "method",
        ChunkKind::Class => "class",
        ChunkKind::Module => "module",
    }
}

fn str_to_chunk_kind(s: &str) -> Option<ChunkKind> {
    match s {
        "function" => Some(ChunkKind::Function),
        "method" => Some(ChunkKind::Method),
        "class" => Some(ChunkKind::Class),
        "module" => Some(ChunkKind::Module),
        _ => None,
    }
}

/// Pack a vector of `f32` into little-endian bytes. Reverse of
/// [`decode_embedding`].
fn encode_embedding(vec: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vec.len() * 4);
    for f in vec {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Decode a packed-`f32` BLOB. Returns `None` if the byte length does
/// not match `expected_dim * 4`.
fn decode_embedding(blob: &[u8], expected_dim: u32) -> Option<Vec<f32>> {
    let expected_bytes = (expected_dim as usize).checked_mul(4)?;
    if blob.len() != expected_bytes {
        return None;
    }
    let mut out = Vec::with_capacity(expected_dim as usize);
    for chunk in blob.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().ok()?;
        out.push(f32::from_le_bytes(arr));
    }
    Some(out)
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .fold(0.0_f32, |acc, (x, y)| acc + x * y)
}

fn vector_norm(v: &[f32]) -> f32 {
    let sum_sq: f32 = v.iter().map(|x| x * x).sum();
    sum_sq.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-chunk-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        // Seed: project + file rows so FKs are satisfied.
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO project_files (id, project_id, path, language, size_bytes, file_type, sha256, created_at, updated_at) \
             VALUES ('f1', 'p1', 'src/x.ts', 'typescript', 0, 'source', 'h', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed file");
        (pool, path)
    }

    fn sample_chunk(name: &str, content: &str) -> Chunk {
        Chunk {
            kind: ChunkKind::Function,
            name: name.to_string(),
            start_line: 1,
            end_line: 5,
            content: content.to_string(),
            token_count: 10,
            oversize: false,
        }
    }

    fn make_insert(project_id: &str, file_id: &str, chunk: Chunk, vec: Vec<f32>) -> ChunkInsert {
        let dim = u32::try_from(vec.len()).expect("dim fits in u32");
        ChunkInsert {
            project_id: project_id.to_string(),
            file_id: file_id.to_string(),
            chunk,
            embedding: vec,
            embedding_dim: dim,
            embedding_provider: "ollama-nomic-embed-text".to_string(),
            embedding_model: "nomic-embed-text".to_string(),
        }
    }

    #[test]
    fn encode_decode_roundtrip_preserves_values() {
        let original = vec![0.0_f32, 1.5, -2.25, std::f32::consts::PI];
        let bytes = encode_embedding(&original);
        let dim = u32::try_from(original.len()).expect("dim");
        let back = decode_embedding(&bytes, dim).expect("decode");
        assert_eq!(back, original);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        let bytes = vec![0u8; 12]; // 3 floats
        assert!(decode_embedding(&bytes, 4).is_none());
    }

    #[test]
    fn empty_insert_returns_empty_vec_without_pool_call() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            let pool = init_pool_at(&tmp_db()).await.expect("pool");
            let result = insert_batch(&pool, Vec::new()).await.expect("ok");
            assert!(result.is_empty());
        });
    }

    #[tokio::test]
    async fn insert_then_search_returns_inserted_chunks() {
        let (pool, path) = seed_pool().await;

        let chunk = sample_chunk("add", "function add(a, b) { return a + b; }");
        let insert = make_insert("p1", "f1", chunk, vec![1.0, 0.0, 0.0]);
        let ids = insert_batch(&pool, vec![insert]).await.expect("insert");
        assert_eq!(ids.len(), 1);

        let hits = search_similar(
            &pool,
            "p1",
            "ollama-nomic-embed-text",
            3,
            &[1.0, 0.0, 0.0],
            5,
        )
        .await
        .expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "add");
        // Cosine of identical unit vectors == 1.
        assert!((hits[0].similarity - 1.0).abs() < 1e-5);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn search_orders_results_by_similarity_descending() {
        let (pool, path) = seed_pool().await;

        let close = make_insert(
            "p1",
            "f1",
            sample_chunk("close", "..."),
            vec![1.0, 0.1, 0.0],
        );
        let far = make_insert("p1", "f1", sample_chunk("far", "..."), vec![0.0, 1.0, 0.0]);
        insert_batch(&pool, vec![close, far]).await.expect("insert");

        let hits = search_similar(
            &pool,
            "p1",
            "ollama-nomic-embed-text",
            3,
            &[1.0, 0.0, 0.0],
            5,
        )
        .await
        .expect("search");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "close");
        assert_eq!(hits[1].name, "far");
        assert!(hits[0].similarity > hits[1].similarity);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn search_filters_by_provider_and_dim() {
        let (pool, path) = seed_pool().await;

        let mut a = make_insert("p1", "f1", sample_chunk("a", "..."), vec![1.0, 0.0]);
        a.embedding_provider = "ollama-nomic-embed-text".into();
        let mut b = make_insert("p1", "f1", sample_chunk("b", "..."), vec![1.0, 0.0]);
        b.embedding_provider = "openai-text-embedding-3-small".into();
        insert_batch(&pool, vec![a, b]).await.expect("insert");

        let hits = search_similar(&pool, "p1", "ollama-nomic-embed-text", 2, &[1.0, 0.0], 5)
            .await
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "a");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn search_rejects_query_with_wrong_dim() {
        let (pool, path) = seed_pool().await;
        let err = search_similar(&pool, "p1", "ollama-nomic-embed-text", 3, &[1.0, 0.0], 5)
            .await
            .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn insert_rejects_embedding_length_mismatch() {
        let (pool, path) = seed_pool().await;

        let mut bad = make_insert("p1", "f1", sample_chunk("bad", "..."), vec![1.0, 0.0]);
        bad.embedding_dim = 3; // claim 3 dims but vector is 2
        let err = insert_batch(&pool, vec![bad])
            .await
            .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn count_for_tuple_returns_inserted_count() {
        let (pool, path) = seed_pool().await;

        let inserts: Vec<_> = (0..3)
            .map(|i| {
                let chunk = sample_chunk(&format!("f{i}"), "...");
                let i16_i = i16::try_from(i).expect("i fits");
                make_insert("p1", "f1", chunk, vec![f32::from(i16_i), 0.0])
            })
            .collect();
        insert_batch(&pool, inserts).await.expect("insert");

        let count = count_for_tuple(&pool, "p1", "ollama-nomic-embed-text", 2)
            .await
            .expect("count");
        assert_eq!(count, 3);

        let other_dim = count_for_tuple(&pool, "p1", "ollama-nomic-embed-text", 768)
            .await
            .expect("count");
        assert_eq!(other_dim, 0);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn search_top_k_clamped_to_cap() {
        let (pool, path) = seed_pool().await;

        let inserts: Vec<_> = (0..5)
            .map(|i| {
                let chunk = sample_chunk(&format!("f{i}"), "...");
                let i16_i = i16::try_from(i).expect("i fits");
                make_insert("p1", "f1", chunk, vec![1.0_f32, f32::from(i16_i) * 0.1])
            })
            .collect();
        insert_batch(&pool, inserts).await.expect("insert");

        let hits = search_similar(
            &pool,
            "p1",
            "ollama-nomic-embed-text",
            2,
            &[1.0, 0.0],
            10_000,
        )
        .await
        .expect("search");
        // We only have 5 rows, but the cap also bounds top_k.
        assert!(hits.len() <= SEARCH_TOP_K_CAP);
        assert_eq!(hits.len(), 5);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn zero_query_vector_returns_empty_without_nan() {
        let (pool, path) = seed_pool().await;

        let insert = make_insert("p1", "f1", sample_chunk("a", "..."), vec![1.0, 0.0]);
        insert_batch(&pool, vec![insert]).await.expect("insert");

        let hits = search_similar(&pool, "p1", "ollama-nomic-embed-text", 2, &[0.0, 0.0], 5)
            .await
            .expect("search");
        assert!(hits.is_empty());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
