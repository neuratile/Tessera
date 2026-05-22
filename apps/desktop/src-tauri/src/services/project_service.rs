//! Project lifecycle service.
//!
//! Per `rules.md` §4.2: business logic for project CRUD. Delegates
//! to `project_repo` for persistence and validates inputs at the
//! service boundary.

use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::repositories::project_repo::{self, Project, ProjectInsert};

/// Default page size for list endpoints when the caller does not
/// supply one. Keeps the IPC payload bounded so the renderer cannot
/// receive thousands of projects in a single round-trip.
pub const DEFAULT_PAGE_LIMIT: i64 = 100;
/// Hard cap on caller-supplied page sizes.
pub const MAX_PAGE_LIMIT: i64 = 1_000;

pub async fn create_project(
    pool: &SqlitePool,
    name: String,
    root_path: String,
) -> AppResult<Project> {
    let id = project_repo::insert(pool, ProjectInsert { name, root_path }).await?;
    project_repo::fetch(pool, &id).await
}

pub async fn list_projects(
    pool: &SqlitePool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> AppResult<Vec<Project>> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let offset = offset.unwrap_or(0).max(0);
    project_repo::list_for_user(
        pool,
        "00000000-0000-4000-8000-000000000001",
        limit,
        offset,
    )
    .await
}

pub async fn get_project(pool: &SqlitePool, id: &str) -> AppResult<Project> {
    project_repo::fetch(pool, id).await
}

pub async fn delete_project(pool: &SqlitePool, id: &str) -> AppResult<()> {
    project_repo::delete(pool, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-psvc-{}.db", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn create_and_list_round_trips() {
        let path = tmp_db();
        let pool = crate::db::init_pool_at(&path).await.expect("pool");

        let project = create_project(&pool, "Test".into(), "/tmp/test".into())
            .await
            .expect("create");
        assert_eq!(project.name, "Test");

        let list = list_projects(&pool, None, None).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, project.id);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn delete_removes_from_list() {
        let path = tmp_db();
        let pool = crate::db::init_pool_at(&path).await.expect("pool");

        let project = create_project(&pool, "Gone".into(), "/tmp/gone".into())
            .await
            .expect("create");
        delete_project(&pool, &project.id).await.expect("delete");

        let list = list_projects(&pool, None, None).await.expect("list");
        assert!(list.is_empty());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
