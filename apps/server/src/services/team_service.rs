//! Team management business logic.

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::team::{CreateTeam, Team};
use crate::models::user::UserProfile;

/// Generate a unique invite code.
///
/// 20 hex chars drawn from two UUIDv4s (~76 bits of entropy). Joining with a
/// code grants full team membership, so the space must withstand online
/// brute force; 8 chars (32 bits) was guessable.
fn generate_invite_code() -> String {
    let a = Uuid::new_v4().simple().to_string();
    let b = Uuid::new_v4().simple().to_string();
    format!("{}{}", &a[..10], &b[..10]).to_uppercase()
}

/// Create a new team and add the creator as an admin member.
pub async fn create_team(pool: &PgPool, creator_id: Uuid, payload: CreateTeam) -> ApiResult<Team> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::Validation("team name cannot be empty".into()));
    }

    let mut tx = pool.begin().await?;

    let team_id = Uuid::new_v4();
    let invite_code = generate_invite_code();
    let now = Utc::now();

    // Insert the team
    let team = sqlx::query_as::<_, Team>(
        r#"
        INSERT INTO teams (id, name, description, invite_code, created_by, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, name, description, invite_code, created_by, created_at, updated_at
        "#,
    )
    .bind(team_id)
    .bind(name)
    .bind(payload.description.as_deref())
    .bind(invite_code)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    // Add creator as admin member
    let member_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO team_members (id, team_id, user_id, role, joined_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(member_id)
    .bind(team_id)
    .bind(creator_id)
    .bind("admin")
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(team)
}

/// Join an existing team using its invite code.
pub async fn join_team(pool: &PgPool, user_id: Uuid, invite_code: &str) -> ApiResult<Team> {
    let code = invite_code.trim().to_uppercase();
    if code.is_empty() {
        return Err(ApiError::Validation("invite code cannot be empty".into()));
    }

    let team = sqlx::query_as::<_, Team>(
        "SELECT id, name, description, invite_code, created_by, created_at, updated_at FROM teams WHERE invite_code = $1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("invalid invite code".into()))?;

    // Check if already a member
    let existing = sqlx::query("SELECT id FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team.id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

    if existing.is_some() {
        return Err(ApiError::Validation("already a member of this team".into()));
    }

    let member_id = Uuid::new_v4();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO team_members (id, team_id, user_id, role, joined_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(member_id)
    .bind(team.id)
    .bind(user_id)
    .bind("member")
    .bind(now)
    .execute(pool)
    .await?;

    Ok(team)
}

/// List all teams the user belongs to.
pub async fn list_teams(pool: &PgPool, user_id: Uuid) -> ApiResult<Vec<Team>> {
    let teams = sqlx::query_as::<_, Team>(
        r#"
        SELECT t.id, t.name, t.description, t.invite_code, t.created_by, t.created_at, t.updated_at
        FROM teams t
        INNER JOIN team_members tm ON tm.team_id = t.id
        WHERE tm.user_id = $1
        ORDER BY t.name ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(teams)
}

/// Get team by ID, verifying user is a member.
pub async fn get_team(pool: &PgPool, user_id: Uuid, team_id: Uuid) -> ApiResult<Team> {
    // Check membership
    let _ = check_membership(pool, user_id, team_id).await?;

    let team = sqlx::query_as::<_, Team>(
        "SELECT id, name, description, invite_code, created_by, created_at, updated_at FROM teams WHERE id = $1",
    )
    .bind(team_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("team not found".into()))?;

    Ok(team)
}

/// Detailed team member info with profiles.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberInfo {
    pub id: Uuid,
    pub team_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub joined_at: chrono::DateTime<Utc>,
    pub user: UserProfile,
}

/// List all members of a team.
pub async fn list_members(pool: &PgPool, user_id: Uuid, team_id: Uuid) -> ApiResult<Vec<TeamMemberInfo>> {
    // Check membership
    let _ = check_membership(pool, user_id, team_id).await?;

    #[derive(sqlx::FromRow)]
    struct MemberRow {
        member_id: Uuid,
        team_id: Uuid,
        role: String,
        joined_at: chrono::DateTime<Utc>,
        user_id: Uuid,
        email: String,
        display_name: String,
        avatar_url: Option<String>,
        user_created: chrono::DateTime<Utc>,
        user_updated: chrono::DateTime<Utc>,
    }

    // Fetch members joined with user profiles
    let members = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT tm.id as member_id, tm.team_id, tm.role, tm.joined_at,
               u.id as user_id, u.email, u.display_name, u.avatar_url, u.created_at as user_created, u.updated_at as user_updated
        FROM team_members tm
        INNER JOIN users u ON u.id = tm.user_id
        WHERE tm.team_id = $1
        ORDER BY u.display_name ASC
        "#,
    )
    .bind(team_id)
    .fetch_all(pool)
    .await?;

    let result = members
        .into_iter()
        .map(|row| TeamMemberInfo {
            id: row.member_id,
            team_id: row.team_id,
            user_id: row.user_id,
            role: row.role,
            joined_at: row.joined_at,
            user: UserProfile {
                id: row.user_id,
                email: row.email,
                display_name: row.display_name,
                avatar_url: row.avatar_url,
                created_at: row.user_created,
                updated_at: row.user_updated,
            },
        })
        .collect();

    Ok(result)
}

/// Remove a member from a team.
pub async fn remove_member(pool: &PgPool, requester_id: Uuid, team_id: Uuid, user_id_to_remove: Uuid) -> ApiResult<()> {
    let requester_role = check_membership(pool, requester_id, team_id).await?;
    
    // Non-admins can only remove themselves
    if requester_role != "admin" && requester_id != user_id_to_remove {
        return Err(ApiError::Forbidden("only admins can remove other members".into()));
    }

    // Admins cannot remove themselves unless there are other admins, or they delete the team?
    // For simplicity, just prevent removing the last admin.
    if user_id_to_remove == requester_id && requester_role == "admin" {
        let admin_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM team_members WHERE team_id = $1 AND role = 'admin'",
        )
        .bind(team_id)
        .fetch_one(pool)
        .await?;

        if admin_count <= 1 {
            return Err(ApiError::Validation("cannot remove the last admin from the team".into()));
        }
    }

    sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team_id)
        .bind(user_id_to_remove)
        .execute(pool)
        .await?;

    Ok(())
}

/// Update member role.
pub async fn update_member_role(
    pool: &PgPool,
    requester_id: Uuid,
    team_id: Uuid,
    user_id_to_update: Uuid,
    new_role: &str,
) -> ApiResult<()> {
    if new_role != "admin" && new_role != "member" && new_role != "viewer" {
        return Err(ApiError::Validation("invalid role".into()));
    }

    let requester_role = check_membership(pool, requester_id, team_id).await?;
    if requester_role != "admin" {
        return Err(ApiError::Forbidden("only admins can update member roles".into()));
    }

    // Prevent demoting the last admin — mirrors the guard in remove_member.
    // Otherwise the team would be left with no one able to manage it.
    let target_role = check_membership(pool, user_id_to_update, team_id).await?;
    if target_role == "admin" && new_role != "admin" {
        let admin_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM team_members WHERE team_id = $1 AND role = 'admin'",
        )
        .bind(team_id)
        .fetch_one(pool)
        .await?;

        if admin_count <= 1 {
            return Err(ApiError::Validation("cannot demote the last admin of the team".into()));
        }
    }

    sqlx::query("UPDATE team_members SET role = $1 WHERE team_id = $2 AND user_id = $3")
        .bind(new_role)
        .bind(team_id)
        .bind(user_id_to_update)
        .execute(pool)
        .await?;

    Ok(())
}

/// Helper: check if a user is a member of a team and return their role.
pub async fn check_membership(pool: &PgPool, user_id: Uuid, team_id: Uuid) -> ApiResult<String> {
    let member = sqlx::query("SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

    match member {
        Some(row) => {
            use sqlx::Row;
            let role: String = row.get("role");
            Ok(role)
        }
        None => Err(ApiError::Forbidden("access denied: not a member of this team".into())),
    }
}
