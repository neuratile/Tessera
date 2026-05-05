//! Project file repository — persistence for discovered files.
//!
//! Per `rules.md` §4.2 + §2.3: all SQL for `project_files` lives here.
//! The analysis service calls these functions after file discovery.

use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct ProjectFileInsert {
    pub project_id: String,
    pub path: String,
    pub language: Option<String>,
    pub size_bytes: i64,
    pub file_type: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub id: String,
    pub project_id: String,
    pub path: String,
    pub language: Option<String>,
    pub size_bytes: i64,
    pub file_type: String,
    pub sha256: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Insert a batch of files in a single transaction.
///
/// # Errors
///
/// - `AppError::InvalidInput` if any file has an empty path.
/// - `AppError::Database` for SQLx-level failures.
pub async fn insert_batch(
    pool: &SqlitePool,
    files: Vec<ProjectFileInsert>,
) -> AppResult<Vec<String>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    for f in &files {
        if f.path.trim().is_empty() {
            return Err(AppError::InvalidInput("file path is empty".into()));
        }
    }

    let now = Utc::now().to_rfc3339();
    let mut ids = Vec::with_capacity(files.len());
    let mut tx = pool.begin().await?;

    for f in files {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO project_files \
             (id, project_id, path, language, size_bytes, file_type, sha256, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&f.project_id)
        .bind(&f.path)
        .bind(&f.language)
        .bind(f.size_bytes)
        .bind(&f.file_type)
        .bind(&f.sha256)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        ids.push(id);
    }

    tx.commit().await?;
    Ok(ids)
}

pub async fn list_for_project(pool: &SqlitePool, project_id: &str) -> AppResult<Vec<ProjectFile>> {
    let rows: Vec<ProjectFileRow> = sqlx::query_as(
        "SELECT id, project_id, path, language, size_bytes, file_type, \
                sha256, created_at, updated_at \
         FROM project_files WHERE project_id = ? ORDER BY path ASC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(decode_row).collect())
}

pub async fn delete_for_project(pool: &SqlitePool, project_id: &str) -> AppResult<u64> {
    let result = sqlx::query("DELETE FROM project_files WHERE project_id = ?")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

type ProjectFileRow = (
    String,         // id
    String,         // project_id
    String,         // path
    Option<String>, // language
    i64,            // size_bytes
    String,         // file_type
    String,         // sha256
    String,         // created_at
    String,         // updated_at
);

fn decode_row(row: ProjectFileRow) -> ProjectFile {
    let (id, project_id, path, language, size_bytes, file_type, sha256, created_at, updated_at) =
        row;
    ProjectFile {
        id,
        project_id,
        path,
        language,
        size_bytes,
        file_type,
        sha256,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use std::path::PathBuf;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-pf-{}.db", Uuid::new_v4()))
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

    #[tokio::test]
    async fn insert_batch_then_list() {
        let (pool, path) = seed_pool().await;
        let files = vec![
            ProjectFileInsert {
                project_id: "p1".into(),
                path: "src/main.rs".into(),
                language: Some("rust".into()),
                size_bytes: 1024,
                file_type: "source".into(),
                sha256: "abc123".into(),
            },
            ProjectFileInsert {
                project_id: "p1".into(),
                path: "README.md".into(),
                language: None,
                size_bytes: 256,
                file_type: "documentation".into(),
                sha256: "def456".into(),
            },
        ];
        let ids = insert_batch(&pool, files).await.expect("insert");
        assert_eq!(ids.len(), 2);

        let listed = list_for_project(&pool, "p1").await.expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].path, "README.md");
        assert_eq!(listed[1].path, "src/main.rs");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn insert_batch_rejects_empty_path() {
        let (pool, path) = seed_pool().await;
        let files = vec![ProjectFileInsert {
            project_id: "p1".into(),
            path: "   ".into(),
            language: None,
            size_bytes: 0,
            file_type: "source".into(),
            sha256: "aaa".into(),
        }];
        let err = insert_batch(&pool, files).await.expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn delete_for_project_clears_files() {
        let (pool, path) = seed_pool().await;
        let files = vec![ProjectFileInsert {
            project_id: "p1".into(),
            path: "a.rs".into(),
            language: None,
            size_bytes: 10,
            file_type: "source".into(),
            sha256: "x".into(),
        }];
        insert_batch(&pool, files).await.expect("insert");
        let deleted = delete_for_project(&pool, "p1").await.expect("delete");
        assert_eq!(deleted, 1);

        let remaining = list_for_project(&pool, "p1").await.expect("list");
        assert!(remaining.is_empty());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn empty_batch_returns_empty_vec() {
        let (pool, path) = seed_pool().await;
        let ids = insert_batch(&pool, Vec::new()).await.expect("empty ok");
        assert!(ids.is_empty());
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
