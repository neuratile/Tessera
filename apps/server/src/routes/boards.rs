//! Board and column routes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::board::{Board, BoardColumn, CreateBoard, UpdateBoard};
use crate::services::board_service;
use crate::AppState;
use crate::middleware::auth::AuthUser;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColumnInput {
    name: String,
    color: String,
    wip_limit: Option<i32>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColumnUpdateInput {
    name: Option<String>,
    color: Option<String>,
    // Option<Option<...>>: explicit null clears the WIP limit, omission
    // leaves it unchanged (see models::double_option).
    #[serde(default, deserialize_with = "crate::models::double_option")]
    wip_limit: Option<Option<i32>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderInput {
    column_ids: Vec<Uuid>,
}

async fn list_boards(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(team_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Board>>> {
    let boards = board_service::list_boards(&state.db, auth.user_id, team_id).await?;
    Ok(Json(boards))
}

async fn create_board(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(team_id): Path<Uuid>,
    Json(payload): Json<CreateBoard>,
) -> ApiResult<(StatusCode, Json<Board>)> {
    let board = board_service::create_board(&state.db, auth.user_id, team_id, payload).await?;
    Ok((StatusCode::CREATED, Json(board)))
}

async fn get_board(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
) -> ApiResult<Json<Board>> {
    let board = board_service::get_board(&state.db, auth.user_id, board_id).await?;
    Ok(Json(board))
}

async fn update_board(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
    Json(payload): Json<UpdateBoard>,
) -> ApiResult<Json<Board>> {
    let board = board_service::update_board(&state.db, auth.user_id, board_id, payload).await?;
    Ok(Json(board))
}

async fn delete_board(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    board_service::delete_board(&state.db, auth.user_id, board_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_columns(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
) -> ApiResult<Json<Vec<BoardColumn>>> {
    let columns = board_service::list_columns(&state.db, auth.user_id, board_id).await?;
    Ok(Json(columns))
}

async fn create_column(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
    Json(payload): Json<ColumnInput>,
) -> ApiResult<(StatusCode, Json<BoardColumn>)> {
    let column = board_service::create_column(
        &state.db,
        auth.user_id,
        board_id,
        &payload.name,
        &payload.color,
        payload.wip_limit,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(column)))
}

async fn update_column(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(column_id): Path<Uuid>,
    Json(payload): Json<ColumnUpdateInput>,
) -> ApiResult<Json<BoardColumn>> {
    // We need to fetch the existing column to preserve values if they are None in payload
    let current_column = sqlx::query("SELECT name, color, wip_limit FROM board_columns WHERE id = $1")
        .bind(column_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("column not found".into()))?;

    use sqlx::Row;
    let current_name: String = current_column.get("name");
    let current_color: String = current_column.get("color");
    let current_wip_limit: Option<i32> = current_column.get("wip_limit");

    let name = payload.name.as_deref().unwrap_or(&current_name);
    let color = payload.color.as_deref().unwrap_or(&current_color);
    // Outer None = leave unchanged, Some(None) = clear, Some(Some(v)) = set.
    let wip_limit = payload.wip_limit.unwrap_or(current_wip_limit);

    let column = board_service::update_column(&state.db, auth.user_id, column_id, name, color, wip_limit).await?;
    Ok(Json(column))
}

async fn delete_column(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(column_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    board_service::delete_column(&state.db, auth.user_id, column_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reorder_columns(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
    Json(payload): Json<ReorderInput>,
) -> ApiResult<Json<Vec<BoardColumn>>> {
    let columns = board_service::reorder_columns(&state.db, auth.user_id, board_id, payload.column_ids).await?;
    Ok(Json(columns))
}

/// Mount board and column routes.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Team scoped board routes
        .route("/teams/{team_id}/boards", get(list_boards).post(create_board))
        // Board CRUD
        .route("/boards/{id}", get(get_board).patch(update_board).delete(delete_board))
        // Column listing & creation
        .route("/boards/{id}/columns", get(list_columns).post(create_column))
        .route("/boards/{id}/columns/reorder", put(reorder_columns))
        // Column CRUD
        .route("/columns/{id}", delete(delete_column).put(update_column).patch(update_column))
}
