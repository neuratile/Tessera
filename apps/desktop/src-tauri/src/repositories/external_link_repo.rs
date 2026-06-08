//! External links repository — mappings between Tessera artifacts/items and tracker issues.

use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLinkRow {
    pub id: String,
    pub artifact_id: String,
    pub tracker: String,
    pub item_ref: String,
    pub issue_key: String,
    pub issue_url: String,
    pub issue_type: Option<String>,
    pub last_status: Option<String>,
    pub status_fetched_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ExternalLinkUpsert {
    pub artifact_id: String,
    pub tracker: String,
    pub item_ref: String,
    pub issue_key: String,
    pub issue_url: String,
    pub issue_type: Option<String>,
    pub last_status: Option<String>,
}

/// Insert or update an external link based on unique constraint `(artifact_id, tracker, item_ref)`.
pub async fn upsert(pool: &SqlitePool, row: ExternalLinkUpsert) -> AppResult<String> {
    let now = Utc::now().to_rfc3339();

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM external_links WHERE artifact_id = ? AND tracker = ? AND item_ref = ?",
    )
    .bind(&row.artifact_id)
    .bind(&row.tracker)
    .bind(&row.item_ref)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE external_links SET \
             issue_key = ?, issue_url = ?, issue_type = ?, last_status = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(&row.issue_key)
        .bind(&row.issue_url)
        .bind(&row.issue_type)
        .bind(&row.last_status)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        Ok(id)
    } else {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO external_links \
             (id, artifact_id, tracker, item_ref, issue_key, issue_url, issue_type, last_status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&row.artifact_id)
        .bind(&row.tracker)
        .bind(&row.item_ref)
        .bind(&row.issue_key)
        .bind(&row.issue_url)
        .bind(&row.issue_type)
        .bind(&row.last_status)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(id)
    }
}

pub async fn fetch(pool: &SqlitePool, id: &str) -> AppResult<ExternalLinkRow> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, artifact_id, tracker, item_ref, issue_key, issue_url, issue_type, last_status, status_fetched_at, created_at, updated_at \
         FROM external_links WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| AppError::NotFound(format!("external link {id}")))
        .map(decode_row)
}

pub async fn fetch_by_key(
    pool: &SqlitePool,
    tracker: &str,
    issue_key: &str,
) -> AppResult<Option<ExternalLinkRow>> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, artifact_id, tracker, item_ref, issue_key, issue_url, issue_type, last_status, status_fetched_at, created_at, updated_at \
         FROM external_links WHERE tracker = ? AND issue_key = ?"
    )
    .bind(tracker)
    .bind(issue_key)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(decode_row))
}

pub async fn fetch_for_item(
    pool: &SqlitePool,
    artifact_id: &str,
    tracker: &str,
    item_ref: &str,
) -> AppResult<Option<ExternalLinkRow>> {
    let row: Option<RawRow> = sqlx::query_as(
        "SELECT id, artifact_id, tracker, item_ref, issue_key, issue_url, issue_type, last_status, status_fetched_at, created_at, updated_at \
         FROM external_links \
         WHERE artifact_id = ? AND tracker = ? AND item_ref = ?"
    )
    .bind(artifact_id)
    .bind(tracker)
    .bind(item_ref)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(decode_row))
}

pub async fn list_for_artifact(
    pool: &SqlitePool,
    artifact_id: &str,
) -> AppResult<Vec<ExternalLinkRow>> {
    let rows: Vec<RawRow> = sqlx::query_as(
        "SELECT id, artifact_id, tracker, item_ref, issue_key, issue_url, issue_type, last_status, status_fetched_at, created_at, updated_at \
         FROM external_links WHERE artifact_id = ?"
    )
    .bind(artifact_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(decode_row).collect())
}

/// List every external link, newest first.
///
/// Tessera is single-user/local-first, so all rows belong to the one local
/// user; the AI panel uses this to build an artifact→link map across the whole
/// review queue in a single query (avoids an N+1 per-artifact lookup).
pub async fn list_all(pool: &SqlitePool) -> AppResult<Vec<ExternalLinkRow>> {
    let rows: Vec<RawRow> = sqlx::query_as(
        "SELECT id, artifact_id, tracker, item_ref, issue_key, issue_url, issue_type, last_status, status_fetched_at, created_at, updated_at \
         FROM external_links ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(decode_row).collect())
}

pub async fn update_status(pool: &SqlitePool, id: &str, status: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE external_links SET last_status = ?, status_fetched_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(status)
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM external_links WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("external link {id}")));
    }
    Ok(())
}

type RawRow = (
    String,          // id
    String,          // artifact_id
    String,          // tracker
    String,          // item_ref
    String,          // issue_key
    String,          // issue_url
    Option<String>,  // issue_type
    Option<String>,  // last_status
    Option<String>,  // status_fetched_at
    String,          // created_at
    String,          // updated_at
);

fn decode_row(row: RawRow) -> ExternalLinkRow {
    let (
        id,
        artifact_id,
        tracker,
        item_ref,
        issue_key,
        issue_url,
        issue_type,
        last_status,
        status_fetched_at,
        created_at,
        updated_at,
    ) = row;
    ExternalLinkRow {
        id,
        artifact_id,
        tracker,
        item_ref,
        issue_key,
        issue_url,
        issue_type,
        last_status,
        status_fetched_at,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-extlnk-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT OR IGNORE INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed project");

        sqlx::query(
            "INSERT INTO artifacts (id, project_id, artifact_type, title, content_md, structured_data, generation_metadata, status, version, created_at, updated_at) \
             VALUES ('art1', 'p1', 'test_plan', 'Test Plan v1', '# Plan', '{}', '{}', 'draft', 1, ?, ?)"
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed artifact");

        (pool, path)
    }

    #[tokio::test]
    async fn test_upsert_and_fetch_external_links() {
        let (pool, path) = seed_pool().await;

        let id = upsert(
            &pool,
            ExternalLinkUpsert {
                artifact_id: "art1".to_string(),
                tracker: "jira".to_string(),
                item_ref: String::new(),
                issue_key: "PROJ-123".to_string(),
                issue_url: "https://acme.atlassian.net/browse/PROJ-123".to_string(),
                issue_type: Some("Epic".to_string()),
                last_status: Some("To Do".to_string()),
            },
        )
        .await
        .expect("upsert");

        let fetched = fetch(&pool, &id).await.expect("fetch");
        assert_eq!(fetched.artifact_id, "art1");
        assert_eq!(fetched.tracker, "jira");
        assert_eq!(fetched.item_ref, "");
        assert_eq!(fetched.issue_key, "PROJ-123");
        assert_eq!(fetched.issue_url, "https://acme.atlassian.net/browse/PROJ-123");
        assert_eq!(fetched.issue_type.as_deref(), Some("Epic"));
        assert_eq!(fetched.last_status.as_deref(), Some("To Do"));

        let id2 = upsert(
            &pool,
            ExternalLinkUpsert {
                artifact_id: "art1".to_string(),
                tracker: "jira".to_string(),
                item_ref: String::new(),
                issue_key: "PROJ-123".to_string(),
                issue_url: "https://acme.atlassian.net/browse/PROJ-123".to_string(),
                issue_type: Some("Epic".to_string()),
                last_status: Some("In Progress".to_string()),
            },
        )
        .await
        .expect("upsert update");

        assert_eq!(id, id2);

        let fetched2 = fetch(&pool, &id).await.expect("fetch 2");
        assert_eq!(fetched2.last_status.as_deref(), Some("In Progress"));

        let by_key = fetch_by_key(&pool, "jira", "PROJ-123").await.expect("by key");
        assert!(by_key.is_some());
        assert_eq!(by_key.unwrap().id, id);

        let for_item = fetch_for_item(&pool, "art1", "jira", "").await.expect("for item");
        assert!(for_item.is_some());
        assert_eq!(for_item.unwrap().id, id);

        let list = list_for_artifact(&pool, "art1").await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);

        let all = list_all(&pool).await.expect("list all");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);

        update_status(&pool, &id, "Done").await.expect("update status");
        let fetched3 = fetch(&pool, &id).await.expect("fetch 3");
        assert_eq!(fetched3.last_status.as_deref(), Some("Done"));
        assert!(fetched3.status_fetched_at.is_some());

        delete(&pool, &id).await.expect("delete");
        let err = fetch(&pool, &id).await.expect_err("fetch deleted");
        assert_eq!(err.code(), "NOT_FOUND");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
