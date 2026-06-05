//! Issue management business logic.

use chrono::Utc;
use serde::Serialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::issue::{CreateIssue, Issue, MoveIssue, UpdateIssue};
use crate::models::label::Label;
use crate::models::user::UserProfile;
use crate::services::team_service::check_membership;
use crate::services::ws_hub::SharedWsHub;

/// Detailed issue representation expected by the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailedIssue {
    #[serde(flatten)]
    pub issue: Issue,
    pub assignee: Option<UserProfile>,
    pub reporter: UserProfile,
    pub labels: Vec<Label>,
    pub subtask_count: i64,
    pub comment_count: i64,
}

#[derive(sqlx::FromRow)]
struct DetailedIssueRow {
    id: Uuid,
    board_id: Uuid,
    column_id: Uuid,
    sprint_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    issue_key: String,
    issue_type: String,
    title: String,
    description: String,
    priority: String,
    assignee_id: Option<Uuid>,
    reporter_id: Uuid,
    story_points: Option<i32>,
    due_date: Option<chrono::DateTime<Utc>>,
    git_branch: Option<String>,
    position: i32,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    
    // Assignee fields
    assignee_user_id: Option<Uuid>,
    assignee_email: Option<String>,
    assignee_display_name: Option<String>,
    assignee_avatar_url: Option<String>,
    assignee_created_at: Option<chrono::DateTime<Utc>>,
    assignee_updated_at: Option<chrono::DateTime<Utc>>,
    
    // Reporter fields
    reporter_user_id: Uuid,
    reporter_email: String,
    reporter_display_name: String,
    reporter_avatar_url: Option<String>,
    reporter_created_at: chrono::DateTime<Utc>,
    reporter_updated_at: chrono::DateTime<Utc>,
    
    labels_json: serde_json::Value,
    subtask_count: i64,
    comment_count: i64,
}

#[derive(sqlx::FromRow)]
struct CurrentIssue {
    board_id: Uuid,
    title: String,
    description: String,
    priority: String,
    issue_type: String,
    assignee_id: Option<Uuid>,
    sprint_id: Option<Uuid>,
    story_points: Option<i32>,
    due_date: Option<chrono::DateTime<Utc>>,
    git_branch: Option<String>,
}

#[derive(sqlx::FromRow)]
struct IssueMoveInfo {
    board_id: Uuid,
    column_id: Uuid,
    position: i32,
}

/// Helper to fetch a single detailed issue by ID.
pub async fn get_detailed_issue(pool: &PgPool, issue_id: Uuid) -> ApiResult<DetailedIssue> {
    let row = sqlx::query_as::<_, DetailedIssueRow>(
        r#"
        SELECT i.id, i.board_id, i.column_id, i.sprint_id, i.parent_id, i.issue_key, i.issue_type,
               i.title, i.description, i.priority, i.assignee_id, i.reporter_id, i.story_points,
               i.due_date, i.git_branch, i.position, i.created_at, i.updated_at,
               u_assignee.id as assignee_user_id, u_assignee.email as assignee_email, 
               u_assignee.display_name as assignee_display_name, u_assignee.avatar_url as assignee_avatar_url,
               u_assignee.created_at as assignee_created_at, u_assignee.updated_at as assignee_updated_at,
               u_reporter.id as reporter_user_id, u_reporter.email as reporter_email, 
               u_reporter.display_name as reporter_display_name, u_reporter.avatar_url as reporter_avatar_url,
               u_reporter.created_at as reporter_created_at, u_reporter.updated_at as reporter_updated_at,
               COALESCE(
                   (SELECT json_agg(json_build_object('id', l.id, 'board_id', l.board_id, 'name', l.name, 'color', l.color))
                    FROM labels l
                    INNER JOIN issue_labels il ON il.label_id = l.id
                    WHERE il.issue_id = i.id),
                   '[]'::json
               ) as labels_json,
               (SELECT COUNT(*) FROM issues sub WHERE sub.parent_id = i.id) as subtask_count,
               (SELECT COUNT(*) FROM comments c WHERE c.issue_id = i.id) as comment_count
        FROM issues i
        LEFT JOIN users u_assignee ON u_assignee.id = i.assignee_id
        INNER JOIN users u_reporter ON u_reporter.id = i.reporter_id
        WHERE i.id = $1
        "#,
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("issue not found".into()))?;

    let assignee = row.assignee_user_id.map(|id| UserProfile {
        id,
        email: row.assignee_email.unwrap(),
        display_name: row.assignee_display_name.unwrap(),
        avatar_url: row.assignee_avatar_url,
        created_at: row.assignee_created_at.unwrap(),
        updated_at: row.assignee_updated_at.unwrap(),
    });

    let reporter = UserProfile {
        id: row.reporter_user_id,
        email: row.reporter_email,
        display_name: row.reporter_display_name,
        avatar_url: row.reporter_avatar_url,
        created_at: row.reporter_created_at,
        updated_at: row.reporter_updated_at,
    };

    let labels: Vec<Label> = serde_json::from_value(row.labels_json)
        .map_err(|e| ApiError::Internal(format!("failed to deserialize issue labels: {e}")))?;

    Ok(DetailedIssue {
        issue: Issue {
            id: row.id,
            board_id: row.board_id,
            column_id: row.column_id,
            sprint_id: row.sprint_id,
            parent_id: row.parent_id,
            issue_key: row.issue_key,
            issue_type: row.issue_type,
            title: row.title,
            description: row.description,
            priority: row.priority,
            assignee_id: row.assignee_id,
            reporter_id: row.reporter_id,
            story_points: row.story_points,
            due_date: row.due_date,
            git_branch: row.git_branch,
            position: row.position,
            created_at: row.created_at,
            updated_at: row.updated_at,
        },
        assignee,
        reporter,
        labels,
        subtask_count: row.subtask_count,
        comment_count: row.comment_count,
    })
}

/// Create a new issue on a board.
pub async fn create_issue(
    pool: &PgPool,
    ws_hub: &SharedWsHub,
    user_id: Uuid,
    board_id: Uuid,
    payload: CreateIssue,
    label_ids: Option<Vec<Uuid>>,
) -> ApiResult<DetailedIssue> {
    let board_row = sqlx::query("SELECT team_id, key FROM boards WHERE id = $1")
        .bind(board_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::NotFound("board not found".into()))?;

    use sqlx::Row;
    let board_team_id: Uuid = board_row.get("team_id");
    let board_key: String = board_row.get("key");

    // Check membership
    let role = check_membership(pool, user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot create issues".into()));
    }

    let title = payload.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::Validation("issue title cannot be empty".into()));
    }

    // Determine target column
    let column_id = match payload.column_id {
        Some(col_id) => {
            // Verify column belongs to this board
            let column_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM board_columns WHERE id = $1 AND board_id = $2)")
                .bind(col_id)
                .bind(board_id)
                .fetch_one(pool)
                .await?;
            if !column_exists {
                return Err(ApiError::Validation("column does not belong to this board".into()));
            }
            col_id
        }
        None => {
            sqlx::query_scalar("SELECT id FROM board_columns WHERE board_id = $1 ORDER BY position ASC LIMIT 1")
                .bind(board_id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| ApiError::Internal("board has no columns configured".into()))?
        }
    };

    let mut tx = pool.begin().await?;

    // Increment issue counter on board
    let next_counter: i32 = sqlx::query_scalar("UPDATE boards SET issue_counter = issue_counter + 1 WHERE id = $1 RETURNING issue_counter")
        .bind(board_id)
        .fetch_one(&mut *tx)
        .await?;

    let issue_key = format!("{}-{}", board_key, next_counter);

    // Get max position in column
    let max_pos: Option<i32> = sqlx::query_scalar("SELECT MAX(position) FROM issues WHERE column_id = $1")
        .bind(column_id)
        .fetch_one(&mut *tx)
        .await?;
    let position = max_pos.map(|p| p + 1).unwrap_or(0);

    let issue_id = Uuid::new_v4();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO issues (id, board_id, column_id, sprint_id, parent_id, issue_key, issue_type, title, description, priority, assignee_id, reporter_id, story_points, due_date, git_branch, position, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        "#,
    )
    .bind(issue_id)
    .bind(board_id)
    .bind(column_id)
    .bind(payload.sprint_id)
    .bind(payload.parent_id)
    .bind(issue_key)
    .bind(payload.issue_type)
    .bind(title)
    .bind(payload.description)
    .bind(payload.priority)
    .bind(payload.assignee_id)
    .bind(user_id) // reporter
    .bind(payload.story_points)
    .bind(payload.due_date)
    .bind(payload.git_branch)
    .bind(position)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // Create labels if provided
    if let Some(ids) = label_ids {
        for label_id in ids {
            sqlx::query("INSERT INTO issue_labels (issue_id, label_id) VALUES ($1, $2)")
                .bind(issue_id)
                .bind(label_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    // Write activity log
    let log_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO activity_logs (id, issue_id, user_id, action, field, old_value, new_value, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(log_id)
    .bind(issue_id)
    .bind(user_id)
    .bind("created")
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Fetch full detail to return and broadcast
    let detailed = get_detailed_issue(pool, issue_id).await?;
    
    // Broadcast event
    let payload_val = serde_json::to_value(&detailed).unwrap_or(serde_json::Value::Null);
    ws_hub.broadcast(board_id, "issue_created", user_id, payload_val);

    Ok(detailed)
}

/// Update an issue. Logs activity logs for changed fields.
pub async fn update_issue(
    pool: &PgPool,
    ws_hub: &SharedWsHub,
    user_id: Uuid,
    issue_id: Uuid,
    payload: UpdateIssue,
) -> ApiResult<DetailedIssue> {
    let current = sqlx::query_as::<_, CurrentIssue>(
        "SELECT board_id, title, description, priority, issue_type, assignee_id, sprint_id, story_points, due_date, git_branch FROM issues WHERE id = $1"
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("issue not found".into()))?;

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(current.board_id)
        .fetch_one(pool)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let role = check_membership(pool, user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot update issues".into()));
    }

    let mut tx = pool.begin().await?;
    let now = Utc::now();

    // Helper macro to log field activity changes
    macro_rules! log_change {
        ($field:expr, $old:expr, $new:expr) => {
            let old_val = $old;
            let new_val = $new;
            if old_val != new_val {
                let log_id = Uuid::new_v4();
                sqlx::query(
                    r#"
                    INSERT INTO activity_logs (id, issue_id, user_id, action, field, old_value, new_value, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    "#,
                )
                .bind(log_id)
                .bind(issue_id)
                .bind(user_id)
                .bind("updated")
                .bind($field)
                .bind(old_val)
                .bind(new_val)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
        };
    }

    // 1. Title
    if let Some(ref title) = payload.title {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(ApiError::Validation("issue title cannot be empty".into()));
        }
        log_change!("title", Some(current.title.clone()), Some(trimmed.to_string()));
    }
    // 2. Description
    if let Some(ref desc) = payload.description {
        log_change!("description", Some(current.description.clone()), Some(desc.clone()));
    }
    // 3. Priority
    if let Some(ref prio) = payload.priority {
        log_change!("priority", Some(current.priority.clone()), Some(prio.clone()));
    }
    // 4. Issue Type
    if let Some(ref itype) = payload.issue_type {
        log_change!("issue_type", Some(current.issue_type.clone()), Some(itype.clone()));
    }
    // Nullable fields use Option<Option<T>>: outer None = "leave unchanged",
    // Some(None) = "clear", Some(Some(v)) = "set". Resolve effective values once
    // so the activity log and the UPDATE always agree.
    let new_assignee_id = payload.assignee_id.unwrap_or(current.assignee_id);
    let new_sprint_id = payload.sprint_id.unwrap_or(current.sprint_id);
    let new_story_points = payload.story_points.unwrap_or(current.story_points);
    let new_due_date = payload.due_date.unwrap_or(current.due_date);
    let new_git_branch = payload.git_branch.unwrap_or(current.git_branch.clone());

    // 5. Assignee
    if new_assignee_id != current.assignee_id {
        log_change!(
            "assignee_id",
            current.assignee_id.map(|id| id.to_string()),
            new_assignee_id.map(|id| id.to_string())
        );
    }
    // 6. Sprint
    if new_sprint_id != current.sprint_id {
        log_change!(
            "sprint_id",
            current.sprint_id.map(|id| id.to_string()),
            new_sprint_id.map(|id| id.to_string())
        );
    }
    // 7. Story Points
    if new_story_points != current.story_points {
        log_change!(
            "story_points",
            current.story_points.map(|s| s.to_string()),
            new_story_points.map(|s| s.to_string())
        );
    }
    // 8. Due Date
    if new_due_date != current.due_date {
        log_change!(
            "due_date",
            current.due_date.map(|d| d.to_rfc3339()),
            new_due_date.map(|d| d.to_rfc3339())
        );
    }
    // 9. Git Branch
    if new_git_branch != current.git_branch {
        log_change!(
            "git_branch",
            current.git_branch.clone(),
            new_git_branch.clone()
        );
    }

    // Execute update
    let title = payload.title.as_ref().map(|s| s.trim()).unwrap_or(&current.title);
    let description = payload.description.as_ref().unwrap_or(&current.description);
    let priority = payload.priority.as_ref().unwrap_or(&current.priority);
    let issue_type = payload.issue_type.as_ref().unwrap_or(&current.issue_type);

    sqlx::query(
        r#"
        UPDATE issues
        SET title = $1, description = $2, priority = $3, issue_type = $4, assignee_id = $5,
            sprint_id = $6, story_points = $7, due_date = $8, git_branch = $9, updated_at = $10
        WHERE id = $11
        "#,
    )
    .bind(title)
    .bind(description)
    .bind(priority)
    .bind(issue_type)
    .bind(new_assignee_id)
    .bind(new_sprint_id)
    .bind(new_story_points)
    .bind(new_due_date)
    .bind(new_git_branch)
    .bind(now)
    .bind(issue_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let detailed = get_detailed_issue(pool, issue_id).await?;
    let payload_val = serde_json::to_value(&detailed).unwrap_or(serde_json::Value::Null);
    ws_hub.broadcast(current.board_id, "issue_updated", user_id, payload_val);

    Ok(detailed)
}

/// Move an issue's column / position.
pub async fn move_issue(
    pool: &PgPool,
    ws_hub: &SharedWsHub,
    user_id: Uuid,
    issue_id: Uuid,
    payload: MoveIssue,
) -> ApiResult<DetailedIssue> {
    let issue = sqlx::query_as::<_, IssueMoveInfo>(
        "SELECT board_id, column_id, position FROM issues WHERE id = $1"
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("issue not found".into()))?;

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(issue.board_id)
        .fetch_one(pool)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let role = check_membership(pool, user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot move issues".into()));
    }

    // The target column must belong to the issue's own board — otherwise an
    // issue could be transplanted into another board's column.
    let column_on_board: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM board_columns WHERE id = $1 AND board_id = $2)",
    )
    .bind(payload.column_id)
    .bind(issue.board_id)
    .fetch_one(pool)
    .await?;
    if !column_on_board {
        return Err(ApiError::Validation("column does not belong to this board".into()));
    }

    let mut tx = pool.begin().await?;
    let now = Utc::now();

    let old_col_id = issue.column_id;
    let new_col_id = payload.column_id;
    let old_pos = issue.position;
    let new_pos = payload.position;

    if old_col_id == new_col_id {
        // Same column move
        if old_pos != new_pos {
            if old_pos < new_pos {
                // Moving down: shift items in between up (decrement position)
                sqlx::query("UPDATE issues SET position = position - 1 WHERE column_id = $1 AND position > $2 AND position <= $3")
                    .bind(old_col_id)
                    .bind(old_pos)
                    .bind(new_pos)
                    .execute(&mut *tx)
                    .await?;
            } else {
                // Moving up: shift items in between down (increment position)
                sqlx::query("UPDATE issues SET position = position + 1 WHERE column_id = $1 AND position >= $2 AND position < $3")
                    .bind(old_col_id)
                    .bind(new_pos)
                    .bind(old_pos)
                    .execute(&mut *tx)
                    .await?;
            }

            sqlx::query("UPDATE issues SET position = $1, updated_at = $2 WHERE id = $3")
                .bind(new_pos)
                .bind(now)
                .bind(issue_id)
                .execute(&mut *tx)
                .await?;
        }
    } else {
        // Different column move:
        // 1. Shift old column items down (decrement position)
        sqlx::query("UPDATE issues SET position = position - 1 WHERE column_id = $1 AND position > $2")
            .bind(old_col_id)
            .bind(old_pos)
            .execute(&mut *tx)
            .await?;

        // 2. Shift new column items up (increment position)
        sqlx::query("UPDATE issues SET position = position + 1 WHERE column_id = $1 AND position >= $2")
            .bind(new_col_id)
            .bind(new_pos)
            .execute(&mut *tx)
            .await?;

        // 3. Place current item in new position
        sqlx::query("UPDATE issues SET column_id = $1, position = $2, updated_at = $3 WHERE id = $4")
            .bind(new_col_id)
            .bind(new_pos)
            .bind(now)
            .bind(issue_id)
            .execute(&mut *tx)
            .await?;

        // 4. Log status change activity
        let old_col_name: String = sqlx::query_scalar("SELECT name FROM board_columns WHERE id = $1")
            .bind(old_col_id)
            .fetch_one(&mut *tx).await?;
        let new_col_name: String = sqlx::query_scalar("SELECT name FROM board_columns WHERE id = $1")
            .bind(new_col_id)
            .fetch_one(&mut *tx).await?;

        let log_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO activity_logs (id, issue_id, user_id, action, field, old_value, new_value, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(log_id)
        .bind(issue_id)
        .bind(user_id)
        .bind("status_changed")
        .bind(Some("column".to_string()))
        .bind(Some(old_col_name))
        .bind(Some(new_col_name))
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let detailed = get_detailed_issue(pool, issue_id).await?;
    let payload_val = serde_json::to_value(&detailed).unwrap_or(serde_json::Value::Null);
    ws_hub.broadcast(issue.board_id, "issue_moved", user_id, payload_val);

    Ok(detailed)
}

/// Delete an issue.
pub async fn delete_issue(
    pool: &PgPool,
    ws_hub: &SharedWsHub,
    user_id: Uuid,
    issue_id: Uuid,
) -> ApiResult<()> {
    let issue = sqlx::query_as::<_, IssueMoveInfo>(
        "SELECT board_id, column_id, position FROM issues WHERE id = $1"
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("issue not found".into()))?;

    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(issue.board_id)
        .fetch_one(pool)
        .await?;
    let board_team_id: Uuid = board_row.get("team_id");

    let role = check_membership(pool, user_id, board_team_id).await?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("viewers cannot delete issues".into()));
    }

    let mut tx = pool.begin().await?;

    // Shift positions of subsequent items in same column down
    sqlx::query("UPDATE issues SET position = position - 1 WHERE column_id = $1 AND position > $2")
        .bind(issue.column_id)
        .bind(issue.position)
        .execute(&mut *tx)
        .await?;

    // Delete issue
    sqlx::query("DELETE FROM issues WHERE id = $1")
        .bind(issue_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // Broadcast deletion
    let delete_payload = serde_json::json!({ "id": issue_id });
    ws_hub.broadcast(issue.board_id, "issue_deleted", user_id, delete_payload);

    Ok(())
}

/// List detailed issues with filters.
pub async fn list_issues(
    pool: &PgPool,
    user_id: Uuid,
    board_id: Uuid,
    sprint_id: Option<Uuid>,
    column_id: Option<Uuid>,
) -> ApiResult<Vec<DetailedIssue>> {
    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(board_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::NotFound("board not found".into()))?;
    let board_team_id: Uuid = board_row.get("team_id");

    let _ = check_membership(pool, user_id, board_team_id).await?;

    let rows = sqlx::query_as::<_, DetailedIssueRow>(
        r#"
        SELECT i.id, i.board_id, i.column_id, i.sprint_id, i.parent_id, i.issue_key, i.issue_type,
               i.title, i.description, i.priority, i.assignee_id, i.reporter_id, i.story_points,
               i.due_date, i.git_branch, i.position, i.created_at, i.updated_at,
               u_assignee.id as assignee_user_id, u_assignee.email as assignee_email, 
               u_assignee.display_name as assignee_display_name, u_assignee.avatar_url as assignee_avatar_url,
               u_assignee.created_at as assignee_created_at, u_assignee.updated_at as assignee_updated_at,
               u_reporter.id as reporter_user_id, u_reporter.email as reporter_email, 
               u_reporter.display_name as reporter_display_name, u_reporter.avatar_url as reporter_avatar_url,
               u_reporter.created_at as reporter_created_at, u_reporter.updated_at as reporter_updated_at,
               COALESCE(
                   (SELECT json_agg(json_build_object('id', l.id, 'board_id', l.board_id, 'name', l.name, 'color', l.color))
                    FROM labels l
                    INNER JOIN issue_labels il ON il.label_id = l.id
                    WHERE il.issue_id = i.id),
                   '[]'::json
               ) as labels_json,
               (SELECT COUNT(*) FROM issues sub WHERE sub.parent_id = i.id) as subtask_count,
               (SELECT COUNT(*) FROM comments c WHERE c.issue_id = i.id) as comment_count
        FROM issues i
        LEFT JOIN users u_assignee ON u_assignee.id = i.assignee_id
        INNER JOIN users u_reporter ON u_reporter.id = i.reporter_id
        WHERE i.board_id = $1
          AND ($2::uuid IS NULL OR i.sprint_id = $2)
          AND ($3::uuid IS NULL OR i.column_id = $3)
        ORDER BY i.position ASC
        "#,
    )
    .bind(board_id)
    .bind(sprint_id)
    .bind(column_id)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::new();
    for row in rows {
        let assignee = row.assignee_user_id.map(|id| UserProfile {
            id,
            email: row.assignee_email.unwrap(),
            display_name: row.assignee_display_name.unwrap(),
            avatar_url: row.assignee_avatar_url,
            created_at: row.assignee_created_at.unwrap(),
            updated_at: row.assignee_updated_at.unwrap(),
        });

        let reporter = UserProfile {
            id: row.reporter_user_id,
            email: row.reporter_email,
            display_name: row.reporter_display_name,
            avatar_url: row.reporter_avatar_url,
            created_at: row.reporter_created_at,
            updated_at: row.reporter_updated_at,
        };

        let labels: Vec<Label> = serde_json::from_value(row.labels_json)
            .map_err(|e| ApiError::Internal(format!("failed to deserialize issue labels: {e}")))?;

        result.push(DetailedIssue {
            issue: Issue {
                id: row.id,
                board_id: row.board_id,
                column_id: row.column_id,
                sprint_id: row.sprint_id,
                parent_id: row.parent_id,
                issue_key: row.issue_key,
                issue_type: row.issue_type,
                title: row.title,
                description: row.description,
                priority: row.priority,
                assignee_id: row.assignee_id,
                reporter_id: row.reporter_id,
                story_points: row.story_points,
                due_date: row.due_date,
                git_branch: row.git_branch,
                position: row.position,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            assignee,
            reporter,
            labels,
            subtask_count: row.subtask_count,
            comment_count: row.comment_count,
        });
    }

    Ok(result)
}
