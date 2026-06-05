//! Issue routes.

use axum::extract::{FromRequest, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::issue::{CreateIssue, MoveIssue, UpdateIssue};
use crate::services::issue_service::{self, DetailedIssue};
use crate::AppState;
use crate::middleware::auth::AuthUser;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueFilterParams {
    sprint_id: Option<Uuid>,
    column_id: Option<Uuid>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateIssuePayload {
    #[serde(flatten)]
    create: CreateIssue,
    label_ids: Option<Vec<Uuid>>,
}

async fn list_issues(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
    Query(params): Query<IssueFilterParams>,
) -> ApiResult<Json<Vec<DetailedIssue>>> {
    let issues = issue_service::list_issues(
        &state.db,
        auth.user_id,
        board_id,
        params.sprint_id,
        params.column_id,
    )
    .await?;
    Ok(Json(issues))
}

struct AppJson<T>(pub T);

impl<T, S> FromRequest<S> for AppJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, _state: &S) -> Result<Self, Self::Rejection> {
        let body = req.into_body();
        let bytes = axum::body::to_bytes(body, 1024 * 1024)
            .await
            .map_err(|e| ApiError::Validation(format!("failed to read request body: {e}")))?;

        if let Ok(json_str) = std::str::from_utf8(&bytes) {
            // debug level: request bodies contain user content and must not
            // reach production logs at the default INFO level.
            tracing::debug!("Raw request JSON payload: {}", json_str);
        }

        let value = serde_json::from_slice::<T>(&bytes)
            .map_err(|e| ApiError::Validation(format!("JSON parsing failed: {e}")))?;

        Ok(AppJson(value))
    }
}

async fn create_issue(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(board_id): Path<Uuid>,
    AppJson(payload): AppJson<CreateIssuePayload>,
) -> ApiResult<(StatusCode, Json<DetailedIssue>)> {
    let issue = issue_service::create_issue(
        &state.db,
        &state.ws_hub,
        auth.user_id,
        board_id,
        payload.create,
        payload.label_ids,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(issue)))
}

async fn get_issue(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<Json<DetailedIssue>> {
    // We fetch detailed issue, verifying board team membership inside
    let issue = issue_service::get_detailed_issue(&state.db, issue_id).await?;
    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(issue.issue.board_id)
        .fetch_one(&state.db)
        .await?;

    use sqlx::Row;
    let board_team_id: Uuid = board_row.get("team_id");
    let _ = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;
    Ok(Json(issue))
}

async fn update_issue(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(issue_id): Path<Uuid>,
    Json(payload): Json<UpdateIssue>,
) -> ApiResult<Json<DetailedIssue>> {
    let issue = issue_service::update_issue(
        &state.db,
        &state.ws_hub,
        auth.user_id,
        issue_id,
        payload,
    )
    .await?;
    Ok(Json(issue))
}

async fn move_issue(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(issue_id): Path<Uuid>,
    Json(payload): Json<MoveIssue>,
) -> ApiResult<Json<DetailedIssue>> {
    let issue = issue_service::move_issue(
        &state.db,
        &state.ws_hub,
        auth.user_id,
        issue_id,
        payload,
    )
    .await?;
    Ok(Json(issue))
}

async fn delete_issue(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    issue_service::delete_issue(&state.db, &state.ws_hub, auth.user_id, issue_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DetailedActivityLog {
    id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
    action: String,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    user: crate::models::user::UserProfile,
}

async fn list_activity(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(issue_id): Path<Uuid>,
) -> ApiResult<Json<Vec<DetailedActivityLog>>> {
    let issue_row = sqlx::query("SELECT board_id FROM issues WHERE id = $1")
        .bind(issue_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound("issue not found".into()))?;

    use sqlx::Row;
    let issue_board_id: Uuid = issue_row.get("board_id");

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(issue_board_id)
        .fetch_one(&state.db)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let _ = crate::services::team_service::check_membership(&state.db, auth.user_id, board_team_id).await?;

    #[derive(sqlx::FromRow)]
    struct ActivityLogRow {
        id: Uuid,
        issue_id: Uuid,
        user_id: Uuid,
        action: String,
        field: Option<String>,
        old_value: Option<String>,
        new_value: Option<String>,
        created_at: chrono::DateTime<chrono::Utc>,
        user_user_id: Uuid,
        user_email: String,
        user_display_name: String,
        user_avatar_url: Option<String>,
        user_created_at: chrono::DateTime<chrono::Utc>,
        user_updated_at: chrono::DateTime<chrono::Utc>,
    }

    let rows = sqlx::query_as::<_, ActivityLogRow>(
        r#"
        SELECT al.id, al.issue_id, al.user_id, al.action, al.field, al.old_value, al.new_value, al.created_at,
               u.id as user_user_id, u.email as user_email, u.display_name as user_display_name, u.avatar_url as user_avatar_url,
               u.created_at as user_created_at, u.updated_at as user_updated_at
        FROM activity_logs al
        INNER JOIN users u ON u.id = al.user_id
        WHERE al.issue_id = $1
        ORDER BY al.created_at DESC
        "#,
    )
    .bind(issue_id)
    .fetch_all(&state.db)
    .await?;

    let logs = rows
        .into_iter()
        .map(|row| DetailedActivityLog {
            id: row.id,
            issue_id: row.issue_id,
            user_id: row.user_id,
            action: row.action,
            field: row.field,
            old_value: row.old_value,
            new_value: row.new_value,
            created_at: row.created_at,
            user: crate::models::user::UserProfile {
                id: row.user_user_id,
                email: row.user_email,
                display_name: row.user_display_name,
                avatar_url: row.user_avatar_url,
                created_at: row.user_created_at,
                updated_at: row.user_updated_at,
            },
        })
        .collect();

    Ok(Json(logs))
}

/// Mount issue routes.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Board issues listing and creation
        .route("/boards/{board_id}/issues", get(list_issues).post(create_issue))
        // Issue CRUD
        .route("/issues/{id}", get(get_issue).patch(update_issue).delete(delete_issue))
        // Issue move
        .route("/issues/{id}/move", put(move_issue))
        // Issue activity logs
        .route("/issues/{id}/activity", get(list_activity))
}

