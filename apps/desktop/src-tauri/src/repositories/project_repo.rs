//! Project repository — CRUD for the `projects` table.
//!
//! Per `rules.md` §4.2 + §2.3: all SQL for `projects` lives here.
//! Services call these functions; they never construct SQL.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Pending,
    Analyzing,
    Ready,
    Error,
}

impl ProjectStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Analyzing => "analyzing",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }

    #[must_use]
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "analyzing" => Some(Self::Analyzing),
            "ready" => Some(Self::Ready),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectInsert {
    pub name: String,
    pub root_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub root_path: String,
    pub file_count: i64,
    pub total_size_bytes: i64,
    pub status: ProjectStatus,
    pub language_breakdown: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn insert(pool: &SqlitePool, row: ProjectInsert) -> AppResult<String> {
    if row.name.trim().is_empty() {
        return Err(AppError::InvalidInput("project name is empty".into()));
    }
    if row.root_path.trim().is_empty() {
        return Err(AppError::InvalidInput("project root_path is empty".into()));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO projects \
         (id, user_id, name, root_path, file_count, total_size_bytes, status, \
          language_breakdown, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, 0, 'pending', '{}', ?, ?)",
    )
    .bind(&id)
    .bind(DEFAULT_USER_ID)
    .bind(row.name.trim())
    .bind(row.root_path.trim())
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(id)
}

pub async fn fetch(pool: &SqlitePool, id: &str) -> AppResult<Project> {
    let row: Option<ProjectRow> = sqlx::query_as(
        "SELECT id, user_id, name, root_path, file_count, total_size_bytes, \
                status, language_breakdown, created_at, updated_at \
         FROM projects WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| AppError::NotFound(format!("project {id}")))
        .and_then(decode_row)
}

pub async fn list_for_user(pool: &SqlitePool, user_id: &str) -> AppResult<Vec<Project>> {
    let rows: Vec<ProjectRow> = sqlx::query_as(
        "SELECT id, user_id, name, root_path, file_count, total_size_bytes, \
                status, language_breakdown, created_at, updated_at \
         FROM projects WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(decode_row).collect()
}

pub async fn update_stats(
    pool: &SqlitePool,
    id: &str,
    file_count: i64,
    total_size_bytes: i64,
    language_breakdown: &serde_json::Value,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let lang_json = serde_json::to_string(language_breakdown)?;

    let result = sqlx::query(
        "UPDATE projects SET file_count = ?, total_size_bytes = ?, \
         language_breakdown = ?, updated_at = ? WHERE id = ?",
    )
    .bind(file_count)
    .bind(total_size_bytes)
    .bind(&lang_json)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    Ok(())
}

pub async fn update_status(pool: &SqlitePool, id: &str, status: ProjectStatus) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query("UPDATE projects SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    Ok(())
}

type ProjectRow = (
    String, // id
    String, // user_id
    String, // name
    String, // root_path
    i64,    // file_count
    i64,    // total_size_bytes
    String, // status
    String, // language_breakdown (JSON text)
    String, // created_at
    String, // updated_at
);

fn decode_row(row: ProjectRow) -> AppResult<Project> {
    let (
        id,
        user_id,
        name,
        root_path,
        file_count,
        total_size_bytes,
        status_s,
        lang_text,
        created_at,
        updated_at,
    ) = row;

    let status = ProjectStatus::from_str_value(&status_s).ok_or_else(|| {
        AppError::Database(sqlx::Error::Decode(
            format!("unknown project status `{status_s}`").into(),
        ))
    })?;
    let language_breakdown: serde_json::Value = serde_json::from_str(&lang_text)?;

    Ok(Project {
        id,
        user_id,
        name,
        root_path,
        file_count,
        total_size_bytes,
        status,
        language_breakdown,
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
        std::env::temp_dir().join(format!("testing-ide-proj-{}.db", Uuid::new_v4()))
    }

    async fn seed_pool() -> (SqlitePool, PathBuf) {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        (pool, path)
    }

    #[tokio::test]
    async fn insert_then_fetch_round_trips() {
        let (pool, path) = seed_pool().await;
        let id = insert(
            &pool,
            ProjectInsert {
                name: "My Project".into(),
                root_path: "/tmp/my-project".into(),
            },
        )
        .await
        .expect("insert");

        let project = fetch(&pool, &id).await.expect("fetch");
        assert_eq!(project.name, "My Project");
        assert_eq!(project.root_path, "/tmp/my-project");
        assert_eq!(project.status, ProjectStatus::Pending);
        assert_eq!(project.file_count, 0);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn insert_rejects_empty_name() {
        let (pool, path) = seed_pool().await;
        let err = insert(
            &pool,
            ProjectInsert {
                name: "   ".into(),
                root_path: "/tmp".into(),
            },
        )
        .await
        .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn update_status_works() {
        let (pool, path) = seed_pool().await;
        let id = insert(
            &pool,
            ProjectInsert {
                name: "p".into(),
                root_path: "/tmp".into(),
            },
        )
        .await
        .expect("insert");

        update_status(&pool, &id, ProjectStatus::Ready)
            .await
            .expect("update");
        let p = fetch(&pool, &id).await.expect("fetch");
        assert_eq!(p.status, ProjectStatus::Ready);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn delete_removes_project() {
        let (pool, path) = seed_pool().await;
        let id = insert(
            &pool,
            ProjectInsert {
                name: "p".into(),
                root_path: "/tmp".into(),
            },
        )
        .await
        .expect("insert");

        delete(&pool, &id).await.expect("delete");
        let err = fetch(&pool, &id).await.expect_err("must 404");
        assert_eq!(err.code(), "NOT_FOUND");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn list_returns_projects_newest_first() {
        let (pool, path) = seed_pool().await;
        insert(
            &pool,
            ProjectInsert {
                name: "first".into(),
                root_path: "/a".into(),
            },
        )
        .await
        .expect("first");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        insert(
            &pool,
            ProjectInsert {
                name: "second".into(),
                root_path: "/b".into(),
            },
        )
        .await
        .expect("second");

        let list = list_for_user(&pool, DEFAULT_USER_ID).await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "second");
        assert_eq!(list[1].name, "first");

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn project_status_round_trips() {
        let cases = [
            (ProjectStatus::Pending, "pending"),
            (ProjectStatus::Analyzing, "analyzing"),
            (ProjectStatus::Ready, "ready"),
            (ProjectStatus::Error, "error"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(ProjectStatus::from_str_value(expected), Some(variant));
        }
    }
}
