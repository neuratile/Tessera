//! Board management business logic.

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::board::{Board, BoardColumn, CreateBoard, UpdateBoard};
use crate::services::team_service::check_membership;

/// Create a new board under a team and auto-create default columns (To Do, In Progress, In Review, Done).
pub async fn create_board(
    pool: &PgPool,
    user_id: Uuid,
    team_id: Uuid,
    payload: CreateBoard,
) -> ApiResult<Board> {
    // Check membership
    let role = check_membership(pool, user_id, team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot create boards".into()));
    }

    let name = payload.name.trim().to_string();
    let key = payload.key.trim().to_uppercase();
    if name.is_empty() || key.is_empty() {
        return Err(ApiError::Validation("board name and key cannot be empty".into()));
    }

    // Verify key contains only alphabetic characters, and length is 2-10
    if !key.chars().all(|c| c.is_ascii_alphabetic()) || key.len() < 2 || key.len() > 10 {
        return Err(ApiError::Validation("board key must be 2-10 alphabetic characters".into()));
    }

    let mut tx = pool.begin().await?;

    let board_id = Uuid::new_v4();
    let now = Utc::now();

    // Insert board
    let board = sqlx::query_as::<_, Board>(
        r#"
        INSERT INTO boards (id, team_id, name, key, description, board_type, issue_counter, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, team_id, name, key, description, board_type, issue_counter, created_at, updated_at
        "#,
    )
    .bind(board_id)
    .bind(team_id)
    .bind(name)
    .bind(key)
    .bind(payload.description.as_deref())
    .bind(payload.board_type)
    .bind(0)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    // Create default columns; "Done" carries the is_done marker used by
    // sprint completion to decide which issues count as finished.
    let defaults = vec![
        ("To Do", "#94a3b8", 0, false),
        ("In Progress", "#38bdf8", 1, false),
        ("In Review", "#c084fc", 2, false),
        ("Done", "#34d399", 3, true),
    ];

    for (col_name, col_color, pos, is_done) in defaults {
        let col_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO board_columns (id, board_id, name, color, position, wip_limit, is_done)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(col_id)
        .bind(board_id)
        .bind(col_name)
        .bind(col_color)
        .bind(pos)
        .bind(None::<i32>)
        .bind(is_done)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(board)
}

/// List boards in a team.
pub async fn list_boards(pool: &PgPool, user_id: Uuid, team_id: Uuid) -> ApiResult<Vec<Board>> {
    // Check membership
    let _ = check_membership(pool, user_id, team_id).await?;

    let boards = sqlx::query_as::<_, Board>(
        "SELECT id, team_id, name, key, description, board_type, issue_counter, created_at, updated_at FROM boards WHERE team_id = $1 ORDER BY name ASC",
    )
    .bind(team_id)
    .fetch_all(pool)
    .await?;

    Ok(boards)
}

/// Retrieve a board by ID.
pub async fn get_board(pool: &PgPool, user_id: Uuid, board_id: Uuid) -> ApiResult<Board> {
    let board = sqlx::query_as::<_, Board>(
        "SELECT id, team_id, name, key, description, board_type, issue_counter, created_at, updated_at FROM boards WHERE id = $1",
    )
    .bind(board_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("board not found".into()))?;

    // Check membership on the board's team
    let _ = check_membership(pool, user_id, board.team_id).await?;

    Ok(board)
}

/// Update a board.
pub async fn update_board(
    pool: &PgPool,
    user_id: Uuid,
    board_id: Uuid,
    payload: UpdateBoard,
) -> ApiResult<Board> {
    let board = get_board(pool, user_id, board_id).await?;
    let role = check_membership(pool, user_id, board.team_id).await?;
    if role != "admin" {
        return Err(ApiError::Forbidden("only admins can edit board settings".into()));
    }

    let name = payload.name.unwrap_or(board.name);
    // Outer None = leave unchanged, Some(None) = clear, Some(Some(v)) = set.
    let description = payload.description.unwrap_or(board.description);
    let board_type = payload.board_type.unwrap_or(board.board_type);
    let now = Utc::now();

    let updated = sqlx::query_as::<_, Board>(
        r#"
        UPDATE boards
        SET name = $1, description = $2, board_type = $3, updated_at = $4
        WHERE id = $5
        RETURNING id, team_id, name, key, description, board_type, issue_counter, created_at, updated_at
        "#,
    )
    .bind(name)
    .bind(description)
    .bind(board_type)
    .bind(now)
    .bind(board_id)
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

/// Delete a board.
pub async fn delete_board(pool: &PgPool, user_id: Uuid, board_id: Uuid) -> ApiResult<()> {
    let board = get_board(pool, user_id, board_id).await?;
    let role = check_membership(pool, user_id, board.team_id).await?;
    if role != "admin" {
        return Err(ApiError::Forbidden("only admins can delete boards".into()));
    }

    sqlx::query("DELETE FROM boards WHERE id = $1")
        .bind(board_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// List columns on a board.
pub async fn list_columns(pool: &PgPool, user_id: Uuid, board_id: Uuid) -> ApiResult<Vec<BoardColumn>> {
    let board = get_board(pool, user_id, board_id).await?;
    
    let columns = sqlx::query_as::<_, BoardColumn>(
        "SELECT id, board_id, name, color, position, wip_limit, is_done FROM board_columns WHERE board_id = $1 ORDER BY position ASC",
    )
    .bind(board.id)
    .fetch_all(pool)
    .await?;

    Ok(columns)
}

/// Update a board column.
pub async fn update_column(
    pool: &PgPool,
    user_id: Uuid,
    column_id: Uuid,
    name: &str,
    color: &str,
    wip_limit: Option<i32>,
) -> ApiResult<BoardColumn> {
    let column = sqlx::query_as::<_, BoardColumn>(
        "SELECT id, board_id, name, color, position, wip_limit, is_done FROM board_columns WHERE id = $1",
    )
    .bind(column_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("column not found".into()))?;

    let board = get_board(pool, user_id, column.board_id).await?;
    let role = check_membership(pool, user_id, board.team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot modify columns".into()));
    }

    let updated = sqlx::query_as::<_, BoardColumn>(
        r#"
        UPDATE board_columns
        SET name = $1, color = $2, wip_limit = $3
        WHERE id = $4
        RETURNING id, board_id, name, color, position, wip_limit, is_done
        "#,
    )
    .bind(name)
    .bind(color)
    .bind(wip_limit)
    .bind(column_id)
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

/// Create a new board column.
pub async fn create_column(
    pool: &PgPool,
    user_id: Uuid,
    board_id: Uuid,
    name: &str,
    color: &str,
    wip_limit: Option<i32>,
) -> ApiResult<BoardColumn> {
    let board = get_board(pool, user_id, board_id).await?;
    let role = check_membership(pool, user_id, board.team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot create columns".into()));
    }

    let mut tx = pool.begin().await?;
    let max_pos: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(position) FROM board_columns WHERE board_id = $1",
    )
    .bind(board_id)
    .fetch_one(&mut *tx)
    .await?;
    let position = max_pos.map(|p| p + 1).unwrap_or(0);

    let col_id = Uuid::new_v4();
    let column = sqlx::query_as::<_, BoardColumn>(
        r#"
        INSERT INTO board_columns (id, board_id, name, color, position, wip_limit)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, board_id, name, color, position, wip_limit, is_done
        "#,
    )
    .bind(col_id)
    .bind(board_id)
    .bind(name)
    .bind(color)
    .bind(position)
    .bind(wip_limit)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(column)
}

/// Delete a board column.
pub async fn delete_column(pool: &PgPool, user_id: Uuid, column_id: Uuid) -> ApiResult<()> {
    let column = sqlx::query_as::<_, BoardColumn>(
        "SELECT id, board_id, name, color, position, wip_limit, is_done FROM board_columns WHERE id = $1",
    )
    .bind(column_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("column not found".into()))?;

    let board = get_board(pool, user_id, column.board_id).await?;
    let role = check_membership(pool, user_id, board.team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot delete columns".into()));
    }

    let mut tx = pool.begin().await?;

    // Delete first, then close the gap. The UNIQUE (board_id, position)
    // constraint is DEFERRABLE INITIALLY DEFERRED, so intermediate states
    // inside the transaction are allowed; deleting first also keeps the
    // statement-level state conflict-free.
    sqlx::query("DELETE FROM board_columns WHERE id = $1")
        .bind(column_id)
        .execute(&mut *tx)
        .await?;

    // Shift subsequent columns down
    sqlx::query("UPDATE board_columns SET position = position - 1 WHERE board_id = $1 AND position > $2")
        .bind(column.board_id)
        .bind(column.position)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Reorder columns in a board.
pub async fn reorder_columns(
    pool: &PgPool,
    user_id: Uuid,
    board_id: Uuid,
    column_ids: Vec<Uuid>,
) -> ApiResult<Vec<BoardColumn>> {
    let board = get_board(pool, user_id, board_id).await?;
    let role = check_membership(pool, user_id, board.team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot reorder columns".into()));
    }

    let mut tx = pool.begin().await?;

    // Row-by-row position updates produce duplicate positions on intermediate
    // states. UNIQUE (board_id, position) is DEFERRABLE INITIALLY DEFERRED, so
    // the constraint is only checked at COMMIT, where the final permutation is
    // guaranteed to be consistent.
    for (pos, col_id) in column_ids.iter().enumerate() {
        sqlx::query("UPDATE board_columns SET position = $1 WHERE id = $2 AND board_id = $3")
            .bind(pos as i32)
            .bind(col_id)
            .bind(board_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    
    // Return updated columns list
    let columns = list_columns(pool, user_id, board_id).await?;
    Ok(columns)
}

