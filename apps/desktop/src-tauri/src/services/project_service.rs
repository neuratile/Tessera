//! Project lifecycle service.
//!
//! Per `rules.md` §4.2: business logic for project CRUD. Delegates
//! to `project_repo` for persistence and validates inputs at the
//! service boundary.

use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::repositories::project_repo::{self, Project, ProjectInsert};

pub async fn create_project(
    pool: &SqlitePool,
    name: String,
    root_path: String,
) -> AppResult<Project> {
    let id = project_repo::insert(pool, ProjectInsert { name, root_path }).await?;
    project_repo::fetch(pool, &id).await
}

pub async fn list_projects(pool: &SqlitePool) -> AppResult<Vec<Project>> {
    project_repo::list_for_user(pool, "00000000-0000-4000-8000-000000000001").await
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
    use crate::db::init_pool_at;
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

        let list = list_projects(&pool).await.expect("list");
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

        let list = list_projects(&pool).await.expect("list");
        assert!(list.is_empty());

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }
}
