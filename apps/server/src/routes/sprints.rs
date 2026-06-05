//! Sprint routes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::sprint::{CreateSprint, Sprint, UpdateSprint};
use crate::AppState;
use crate::middleware::auth::AuthUser;

async fn list_sprints(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Sprint>>> {
    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(board_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("board not found".into()))?;

    use sqlx::Row;
    let board_team_id: Uuid = board_row.get("team_id");
    let _ = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;

    let sprints = sqlx::query_as::<_, Sprint>(
        "SELECT id, board_id, name, goal, start_date, end_date, status, created_at FROM sprints WHERE board_id = $1 ORDER BY created_at DESC",
    )
    .bind(board_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(sprints))
}

async fn create_sprint(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
    Json(payload): Json<CreateSprint>,
) -> ApiResult<(StatusCode, Json<Sprint>)> {
    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(board_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("board not found".into()))?;

    use sqlx::Row;
    let board_team_id: Uuid = board_row.get("team_id");
    let role = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot create sprints".into()));
    }

    if payload.name.trim().is_empty() {
        return Err(ApiError::Validation("sprint name cannot be empty".into()));
    }

    let sprint_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let sprint = sqlx::query_as::<_, Sprint>(
        r#"
        INSERT INTO sprints (id, board_id, name, goal, start_date, end_date, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, board_id, name, goal, start_date, end_date, status, created_at
        "#,
    )
    .bind(sprint_id)
    .bind(board_id)
    .bind(payload.name.trim())
    .bind(payload.goal.as_deref())
    .bind(payload.start_date)
    .bind(payload.end_date)
    .bind("planned")
    .bind(now)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(sprint)))
}

async fn update_sprint(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(sprint_id): Path<Uuid>,
    Json(payload): Json<UpdateSprint>,
) -> ApiResult<Json<Sprint>> {
    let current_row = sqlx::query("SELECT board_id, name, goal, start_date, end_date, status, created_at FROM sprints WHERE id = $1")
        .bind(sprint_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("sprint not found".into()))?;

    use sqlx::Row;
    let current_board_id: Uuid = current_row.get("board_id");
    let current_name: String = current_row.get("name");
    let current_goal: Option<String> = current_row.get("goal");
    let current_start_date: Option<chrono::DateTime<chrono::Utc>> = current_row.get("start_date");
    let current_end_date: Option<chrono::DateTime<chrono::Utc>> = current_row.get("end_date");

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(current_board_id)
        .fetch_one(&state.db)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let role = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot edit sprints".into()));
    }

    let name = payload.name.unwrap_or(current_name);
    let goal = payload.goal.or(current_goal);
    let start_date = payload.start_date.or(current_start_date);
    let end_date = payload.end_date.or(current_end_date);

    let sprint = sqlx::query_as::<_, Sprint>(
        r#"
        UPDATE sprints
        SET name = $1, goal = $2, start_date = $3, end_date = $4
        WHERE id = $5
        RETURNING id, board_id, name, goal, start_date, end_date, status, created_at
        "#,
    )
    .bind(name.trim())
    .bind(goal.as_deref())
    .bind(start_date)
    .bind(end_date)
    .bind(sprint_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(sprint))
}

async fn start_sprint(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(sprint_id): Path<Uuid>,
) -> ApiResult<Json<Sprint>> {
    let sprint_info_row = sqlx::query("SELECT board_id, status FROM sprints WHERE id = $1")
        .bind(sprint_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("sprint not found".into()))?;

    use sqlx::Row;
    let sprint_info_board_id: Uuid = sprint_info_row.get("board_id");
    let sprint_info_status: String = sprint_info_row.get("status");

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(sprint_info_board_id)
        .fetch_one(&state.db)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let role = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot start sprints".into()));
    }

    if sprint_info_status != "planned" {
        return Err(ApiError::Validation("only planned sprints can be started".into()));
    }

    // The guard and the UPDATE must be atomic: lock the board row first so
    // two concurrent start requests on the same board are serialized and
    // cannot both observe "no active sprint" (TOCTOU).
    let mut tx = state.db.begin().await?;

    sqlx::query("SELECT id FROM boards WHERE id = $1 FOR UPDATE")
        .bind(sprint_info_board_id)
        .execute(&mut *tx)
        .await?;

    let active_exists: i64 = sqlx::query_scalar("SELECT count(*) FROM sprints WHERE board_id = $1 AND status = 'active'")
        .bind(sprint_info_board_id)
        .fetch_one(&mut *tx)
        .await?;

    if active_exists > 0 {
        return Err(ApiError::Validation("another sprint is already active on this board".into()));
    }

    let now = chrono::Utc::now();
    let sprint = sqlx::query_as::<_, Sprint>(
        r#"
        UPDATE sprints
        SET status = 'active', start_date = COALESCE(start_date, $1)
        WHERE id = $2 AND status = 'planned'
        RETURNING id, board_id, name, goal, start_date, end_date, status, created_at
        "#,
    )
    .bind(now)
    .bind(sprint_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::Validation("only planned sprints can be started".into()))?;

    tx.commit().await?;

    // Broadcast WebSocket event
    state.ws_hub.broadcast(sprint_info_board_id, "sprint_started", auth.user_id, serde_json::json!({ "id": sprint_id }));

    Ok(Json(sprint))
}

async fn complete_sprint(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(sprint_id): Path<Uuid>,
) -> ApiResult<Json<Sprint>> {
    let sprint_info_row = sqlx::query("SELECT board_id, status FROM sprints WHERE id = $1")
        .bind(sprint_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("sprint not found".into()))?;

    use sqlx::Row;
    let sprint_info_board_id: Uuid = sprint_info_row.get("board_id");
    let sprint_info_status: String = sprint_info_row.get("status");

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(sprint_info_board_id)
        .fetch_one(&state.db)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let role = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot complete sprints".into()));
    }

    if sprint_info_status != "active" {
        return Err(ApiError::Validation("only active sprints can be completed".into()));
    }

    let mut tx = state.db.begin().await?;

    // 1. Move incomplete issues to backlog (sprint_id = NULL)
    // The Done column is marked explicitly via is_done — position is not a
    // safe anchor once users append columns after "Done". Fall back to the
    // highest position only for legacy boards without the flag set.
    let done_column_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM board_columns WHERE board_id = $1 ORDER BY is_done DESC, position DESC LIMIT 1",
    )
    .bind(sprint_info_board_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(done_col_id) = done_column_id {
        sqlx::query("UPDATE issues SET sprint_id = NULL WHERE sprint_id = $1 AND column_id != $2")
            .bind(sprint_id)
            .bind(done_col_id)
            .execute(&mut *tx)
            .await?;
    } else {
        // If no columns are found, move all issues to backlog
        sqlx::query("UPDATE issues SET sprint_id = NULL WHERE sprint_id = $1")
            .bind(sprint_id)
            .execute(&mut *tx)
            .await?;
    }

    // 2. Complete the sprint
    let now = chrono::Utc::now();
    let sprint = sqlx::query_as::<_, Sprint>(
        r#"
        UPDATE sprints
        SET status = 'completed', end_date = COALESCE(end_date, $1)
        WHERE id = $2
        RETURNING id, board_id, name, goal, start_date, end_date, status, created_at
        "#,
    )
    .bind(now)
    .bind(sprint_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    // Broadcast WebSocket event
    state.ws_hub.broadcast(sprint_info_board_id, "sprint_completed", auth.user_id, serde_json::json!({ "id": sprint_id }));

    Ok(Json(sprint))
}

/// Mount sprint routes.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/boards/{board_id}/sprints", get(list_sprints).post(create_sprint))
        .route("/sprints/{id}", put(update_sprint).patch(update_sprint)) // PUT/PATCH
        .route("/sprints/{id}/start", post(start_sprint))
        .route("/sprints/{id}/complete", post(complete_sprint))
}
