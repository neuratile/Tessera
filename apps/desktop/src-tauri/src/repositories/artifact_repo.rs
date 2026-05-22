//! Artifact repository — persistence for AI-generated test artifacts.
//!
//! Per `rules.md` §4.2 + §2.3, this module owns all SQL touching the
//! `artifacts` table and its `generation_metadata` JSON column
//! (added in migration `0002_generation_metadata.sql`).
//!
//! Phase 5 callers (`services::generation_service`) hand in a typed
//! [`ArtifactInsert`] with the structured tool-call output validated
//! against the prompt's JSON Schema; the repository assigns the
//! primary key, timestamps, and serializes the JSON bodies.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Logical artifact kinds the IDE can produce. Mirrors the prompt
/// modules under `crate::prompts` (`context_md_v1`, `test_plan_v1`,
/// `test_cases_v1`, `defect_report_v1`, `bug_report_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    ContextMd,
    TestPlan,
    TestCases,
    DefectReport,
    BugReport,
}

impl ArtifactType {
    /// Stable string used in DB rows and IPC payloads. Mirrors the
    /// kebab/snake-case serde rename so the round-trip stays lossless.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContextMd => "context_md",
            Self::TestPlan => "test_plan",
            Self::TestCases => "test_cases",
            Self::DefectReport => "defect_report",
            Self::BugReport => "bug_report",
        }
    }

    /// Inverse of [`as_str`] used by the repository when reading rows.
    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "context_md" => Some(Self::ContextMd),
            "test_plan" => Some(Self::TestPlan),
            "test_cases" => Some(Self::TestCases),
            "defect_report" => Some(Self::DefectReport),
            "bug_report" => Some(Self::BugReport),
            _ => None,
        }
    }

    /// `kebab-case` form used over the IPC boundary. Mirrors
    /// `GenerationArtifactTypeSchema` in `packages/shared/`. DB storage
    /// stays `snake_case` via [`as_str`]; this is purely the renderer
    /// wire format.
    #[must_use]
    pub fn as_ipc_str(self) -> &'static str {
        match self {
            Self::ContextMd => "context-md",
            Self::TestPlan => "test-plan",
            Self::TestCases => "test-cases",
            Self::DefectReport => "defect-report",
            Self::BugReport => "bug-report",
        }
    }

    /// Inverse of [`as_ipc_str`]. Used by command-layer parsers that
    /// receive `kebab-case` literals from the renderer.
    #[must_use]
    pub fn from_ipc_str(s: &str) -> Option<Self> {
        match s {
            "context-md" => Some(Self::ContextMd),
            "test-plan" => Some(Self::TestPlan),
            "test-cases" => Some(Self::TestCases),
            "defect-report" => Some(Self::DefectReport),
            "bug-report" => Some(Self::BugReport),
            _ => None,
        }
    }
}

/// Lifecycle status of a generated artifact. Mirrors the
/// `ArtifactStatusSchema` in `packages/shared/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Draft,
    InReview,
    Approved,
    Rejected,
}

impl ArtifactStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::InReview => "in_review",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "in_review" => Some(Self::InReview),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// Producer-side traceability: which model + prompt + token budget
/// produced this artifact. Persisted as JSON in
/// `artifacts.generation_metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMetadata {
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// ISO-8601 timestamp of when the LLM call started.
    pub started_at: String,
    /// ISO-8601 timestamp of when the response completed.
    pub completed_at: String,
}

/// Fields a caller must provide to insert an artifact.
#[derive(Debug, Clone)]
pub struct ArtifactInsert {
    pub project_id: String,
    pub artifact_type: ArtifactType,
    pub title: String,
    pub content_md: String,
    pub structured_data: serde_json::Value,
    pub generation_metadata: GenerationMetadata,
    pub parent_id: Option<String>,
}

/// Row returned from [`fetch`]. Carries decoded enum + parsed JSON.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub id: String,
    pub project_id: String,
    pub artifact_type: ArtifactType,
    pub title: String,
    pub content_md: String,
    pub structured_data: serde_json::Value,
    pub generation_metadata: GenerationMetadata,
    pub status: ArtifactStatus,
    pub version: i64,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Insert one artifact and return its assigned id.
///
/// The status defaults to `draft` and version to `1`. Callers that
/// regenerate from feedback supply `parent_id` and the repository
/// bumps the parent chain's max version.
///
/// # Errors
///
/// - [`AppError::InvalidInput`] when `title` or `content_md` are
///   empty after trim.
/// - [`AppError::Serde`] when the structured-data / metadata JSON
///   cannot be re-serialized for storage (effectively impossible for
///   well-formed `serde_json::Value`s but propagated for safety).
/// - [`AppError::Database`] for any SQLx-level failure.
pub async fn insert(pool: &SqlitePool, row: ArtifactInsert) -> AppResult<String> {
    if row.title.trim().is_empty() {
        return Err(AppError::InvalidInput("artifact title is empty".into()));
    }
    if row.content_md.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "artifact content_md is empty".into(),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let structured_text = serde_json::to_string(&row.structured_data)?;
    let metadata_text = serde_json::to_string(&row.generation_metadata)?;
    let parent = row.parent_id.as_deref();

    // Single INSERT ... SELECT statement so version computation and the
    // write happen atomically under SQLite's per-statement write lock —
    // no explicit transaction, no BEGIN IMMEDIATE, no manual ROLLBACK.
    // This is cancellation-safe: if the awaiting future is dropped
    // (e.g. Tauri IPC timeout, window close) the connection returns to
    // the pool with no lingering transaction state because no
    // transaction was ever opened.
    //
    // The trailing `WHERE ? IS NULL OR EXISTS(...)` guards against the
    // caller supplying a parent_id that does not exist; SQLite returns
    // rows_affected = 0 in that case and the caller surface NotFound.
    let result = sqlx::query(
        "INSERT INTO artifacts \
         (id, project_id, artifact_type, title, content_md, structured_data, \
          generation_metadata, status, version, parent_id, created_at, updated_at) \
         SELECT ?, ?, ?, ?, ?, ?, ?, 'draft', \
                CASE WHEN ? IS NULL THEN 1 \
                     ELSE COALESCE(( \
                       WITH RECURSIVE chain(id) AS ( \
                         SELECT id FROM artifacts WHERE id = ? \
                         UNION \
                         SELECT a.id FROM artifacts a JOIN chain c ON a.parent_id = c.id \
                       ) \
                       SELECT MAX(version) FROM artifacts WHERE id IN (SELECT id FROM chain) \
                     ), 0) + 1 \
                END, \
                ?, ?, ? \
         FROM (SELECT 1) AS dummy \
         WHERE ? IS NULL OR EXISTS (SELECT 1 FROM artifacts WHERE id = ?)",
    )
    .bind(&id)
    .bind(&row.project_id)
    .bind(row.artifact_type.as_str())
    .bind(&row.title)
    .bind(&row.content_md)
    .bind(&structured_text)
    .bind(&metadata_text)
    .bind(parent) // CASE WHEN ? IS NULL
    .bind(parent) // recursive CTE seed
    .bind(parent) // parent_id column
    .bind(&now)
    .bind(&now)
    .bind(parent) // WHERE outer
    .bind(parent) // EXISTS check
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        let missing = parent.unwrap_or("<unknown>");
        return Err(AppError::NotFound(format!("artifact {missing}")));
    }
    Ok(id)
}

/// Fetch a single artifact by id, decoding the JSON columns.
///
/// # Errors
///
/// - [`AppError::NotFound`] when the row does not exist.
/// - [`AppError::Serde`] when the stored JSON columns are malformed
///   (corruption-detection — the producer side guarantees valid JSON
///   on write so this should never trip in practice).
/// - [`AppError::Database`] for SQLx-level failures.
pub async fn fetch(pool: &SqlitePool, id: &str) -> AppResult<Artifact> {
    let row: Option<ArtifactRow> = sqlx::query_as(
        "SELECT id, project_id, artifact_type, title, content_md, structured_data, \
                generation_metadata, status, version, parent_id, created_at, updated_at \
         FROM artifacts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let row = row.ok_or_else(|| AppError::NotFound(format!("artifact {id}")))?;
    decode_row(row)
}

/// List artifacts for a project, newest first.
///
/// # Errors
///
/// Returns [`AppError::Database`] for SQLx-level failures and
/// [`AppError::Serde`] for any row whose JSON columns fail to decode.
pub async fn list_for_project(
    pool: &SqlitePool,
    project_id: &str,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<Artifact>> {
    let rows: Vec<ArtifactRow> = sqlx::query_as(
        "SELECT id, project_id, artifact_type, title, content_md, structured_data, \
                generation_metadata, status, version, parent_id, created_at, updated_at \
         FROM artifacts WHERE project_id = ? ORDER BY created_at DESC \
         LIMIT ? OFFSET ?",
    )
    .bind(project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(decode_row).collect()
}

/// Update the lifecycle status of an existing artifact and bump
/// `updated_at`. Returns `Ok(())` on success.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no row matches `id`.
/// - [`AppError::Database`] for SQLx-level failures.
pub async fn update_status(pool: &SqlitePool, id: &str, status: ArtifactStatus) -> AppResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query("UPDATE artifacts SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("artifact {id}")));
    }
    Ok(())
}

/// Lightweight projection used by the version-history dropdown in
/// the artifact detail drawer. Excludes the heavy `content_md` /
/// `structured_data` columns so the dropdown can list a long
/// version chain without dragging full Markdown bodies into the
/// renderer just to render menu rows.
#[derive(Debug, Clone)]
pub struct ArtifactVersionRow {
    pub id: String,
    pub version: i64,
    pub status: ArtifactStatus,
    pub title: String,
    pub created_at: String,
    pub parent_id: Option<String>,
}

/// Walk the bidirectional lineage chain (ancestors + self +
/// descendants) for `id`, sorted by `version` ascending so the
/// renderer can show v1 → v2 → … → vN without re-sorting.
///
/// Implemented as a single recursive CTE that grows the visited set
/// in both directions and `UNION` (not `UNION ALL`) so cycles in
/// data — should never happen, but defensively — cannot deadlock
/// the query.
///
/// # Errors
///
/// - [`AppError::NotFound`] when no artifact matches `id`.
/// - [`AppError::Database`] for SQLx-level failures.
/// - [`AppError::Serde`] when the stored `status` column does not
///   match any [`ArtifactStatus`] variant (corruption detection).
pub async fn list_version_chain(
    pool: &SqlitePool,
    id: &str,
) -> AppResult<Vec<ArtifactVersionRow>> {
    let rows: Vec<(String, i64, String, String, String, Option<String>)> = sqlx::query_as(
        "WITH RECURSIVE lineage(id) AS ( \
             SELECT id FROM artifacts WHERE id = ? \
             UNION \
             SELECT a.id FROM artifacts a JOIN lineage l ON a.id = l.parent_id \
             UNION \
             SELECT a.id FROM artifacts a JOIN lineage l ON a.parent_id = l.id \
         ) \
         SELECT id, version, status, title, created_at, parent_id \
         FROM artifacts WHERE id IN (SELECT id FROM lineage) ORDER BY version ASC",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Err(AppError::NotFound(format!("artifact {id}")));
    }

    rows.into_iter()
        .map(|(id, version, status_s, title, created_at, parent_id)| {
            let status = ArtifactStatus::from_str_value(&status_s).ok_or_else(|| {
                AppError::Database(sqlx::Error::Decode(
                    format!("unknown artifact status `{status_s}`").into(),
                ))
            })?;
            Ok(ArtifactVersionRow {
                id,
                version,
                status,
                title,
                created_at,
                parent_id,
            })
        })
        .collect()
}

/// Internal row shape for sqlx decoding. Strings stay strings here;
/// [`decode_row`] does the JSON + enum parsing.
type ArtifactRow = (
    String,         // id
    String,         // project_id
    String,         // artifact_type
    String,         // title
    String,         // content_md
    String,         // structured_data (JSON text)
    String,         // generation_metadata (JSON text)
    String,         // status
    i64,            // version
    Option<String>, // parent_id
    String,         // created_at
    String,         // updated_at
);

fn decode_row(row: ArtifactRow) -> AppResult<Artifact> {
    let (
        id,
        project_id,
        artifact_type_s,
        title,
        content_md,
        structured_text,
        metadata_text,
        status_s,
        version,
        parent_id,
        created_at,
        updated_at,
    ) = row;

    let artifact_type = ArtifactType::from_str_value(&artifact_type_s).ok_or_else(|| {
        AppError::Database(sqlx::Error::Decode(
            format!("unknown artifact_type `{artifact_type_s}`").into(),
        ))
    })?;
    let status = ArtifactStatus::from_str_value(&status_s).ok_or_else(|| {
        AppError::Database(sqlx::Error::Decode(
            format!("unknown artifact status `{status_s}`").into(),
        ))
    })?;
    let structured_data: serde_json::Value = serde_json::from_str(&structured_text)?;
    let generation_metadata: GenerationMetadata = serde_json::from_str(&metadata_text)?;

    Ok(Artifact {
        id,
        project_id,
        artifact_type,
        title,
        content_md,
        structured_data,
        generation_metadata,
        status,
        version,
        parent_id,
        created_at,
        updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-art-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
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
        (pool, path)
    }

    fn sample_metadata() -> GenerationMetadata {
        GenerationMetadata {
            provider: "ollama".into(),
            model: "qwen2.5-coder:7b".into(),
            prompt_version: "test_plan_v1".into(),
            input_tokens: 1000,
            output_tokens: 500,
            started_at: "2026-05-04T16:00:00Z".into(),
            completed_at: "2026-05-04T16:00:30Z".into(),
        }
    }

    fn sample_insert() -> ArtifactInsert {
        ArtifactInsert {
            project_id: "p1".into(),
            artifact_type: ArtifactType::TestPlan,
            title: "Test plan v1".into(),
            content_md: "# Plan\n".into(),
            structured_data: serde_json::json!({ "summary": "demo" }),
            generation_metadata: sample_metadata(),
            parent_id: None,
        }
    }

    #[test]
    fn artifact_type_round_trips_through_serde() {
        let cases = [
            (ArtifactType::ContextMd, "context_md"),
            (ArtifactType::TestPlan, "test_plan"),
            (ArtifactType::TestCases, "test_cases"),
            (ArtifactType::DefectReport, "defect_report"),
            (ArtifactType::BugReport, "bug_report"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(ArtifactType::from_str_value(expected), Some(variant));
        }
    }

    #[test]
    fn artifact_status_round_trips() {
        let cases = [
            (ArtifactStatus::Draft, "draft"),
            (ArtifactStatus::InReview, "in_review"),
            (ArtifactStatus::Approved, "approved"),
            (ArtifactStatus::Rejected, "rejected"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(ArtifactStatus::from_str_value(expected), Some(variant));
        }
    }

    #[tokio::test]
    async fn insert_then_fetch_round_trips_data() {
        let (pool, path) = seed_pool().await;
        let id = insert(&pool, sample_insert()).await.expect("insert");

        let fetched = fetch(&pool, &id).await.expect("fetch");
        assert_eq!(fetched.artifact_type, ArtifactType::TestPlan);
        assert_eq!(fetched.title, "Test plan v1");
        assert_eq!(fetched.status, ArtifactStatus::Draft);
        assert_eq!(fetched.version, 1);
        assert_eq!(fetched.structured_data["summary"], "demo");
        assert_eq!(fetched.generation_metadata.provider, "ollama");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn insert_rejects_empty_title() {
        let (pool, path) = seed_pool().await;
        let mut bad = sample_insert();
        bad.title = "   ".into();
        let err = insert(&pool, bad).await.expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn insert_rejects_empty_content() {
        let (pool, path) = seed_pool().await;
        let mut bad = sample_insert();
        bad.content_md = String::new();
        let err = insert(&pool, bad).await.expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn fetch_missing_returns_not_found() {
        let (pool, path) = seed_pool().await;
        let err = fetch(&pool, "no-such-id").await.expect_err("must error");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn list_returns_artifacts_in_creation_order_descending() {
        let (pool, path) = seed_pool().await;
        let id1 = insert(&pool, sample_insert()).await.expect("first");

        // Distinct timestamps require >= 1s gap with our RFC3339 second
        // precision; sleep is the simplest deterministic way.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let mut second = sample_insert();
        second.title = "second".into();
        let id2 = insert(&pool, second).await.expect("second");

        let list = list_for_project(&pool, "p1", 100, 0).await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2);
        assert_eq!(list[1].id, id1);
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn regenerated_artifact_bumps_version_in_chain() {
        let (pool, path) = seed_pool().await;
        let parent = insert(&pool, sample_insert()).await.expect("parent");

        let mut child = sample_insert();
        child.parent_id = Some(parent.clone());
        child.title = "regen".into();
        let child_id = insert(&pool, child).await.expect("child");

        let child_row = fetch(&pool, &child_id).await.expect("fetch");
        assert_eq!(child_row.version, 2);
        assert_eq!(child_row.parent_id.as_deref(), Some(parent.as_str()));

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn update_status_flips_lifecycle_state() {
        let (pool, path) = seed_pool().await;
        let id = insert(&pool, sample_insert()).await.expect("insert");
        let initial = fetch(&pool, &id).await.expect("initial fetch");
        assert_eq!(initial.status, ArtifactStatus::Draft);

        update_status(&pool, &id, ArtifactStatus::Approved)
            .await
            .expect("approve");
        let approved = fetch(&pool, &id).await.expect("approved fetch");
        assert_eq!(approved.status, ArtifactStatus::Approved);
        assert_ne!(approved.updated_at, initial.updated_at);

        update_status(&pool, &id, ArtifactStatus::Rejected)
            .await
            .expect("reject");
        let rejected = fetch(&pool, &id).await.expect("rejected fetch");
        assert_eq!(rejected.status, ArtifactStatus::Rejected);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn update_status_returns_not_found_for_unknown_id() {
        let (pool, path) = seed_pool().await;
        let err = update_status(&pool, "no-such-id", ArtifactStatus::Approved)
            .await
            .expect_err("must reject unknown id");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
