//! Review persistence only: ownership, atomic progress, immutable source, append-only evidence.
//! No provider calls, filesystem capture, generation, or IPC registration.
mod types;
mod validation;
pub use types::*;

use crate::error::{AppError, AppResult};
use chrono::Utc;
use serde::{de::DeserializeOwned, Serialize};
use sqlx::{SqliteConnection, SqlitePool};
use uuid::Uuid;
use validation::{digest, ensure, invalid, lines, provenance, text};

type RunBindingRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

fn encode<T: Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|_| invalid())
}
fn decode<T: DeserializeOwned>(value: &str) -> AppResult<T> {
    serde_json::from_str(value).map_err(|_| invalid())
}

async fn owned(conn: &mut SqliteConnection, user_id: &str, project_id: Uuid) -> AppResult<()> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM projects WHERE id = ? AND user_id = ?")
            .bind(project_id.to_string())
            .bind(user_id)
            .fetch_one(conn)
            .await?;
    if count.0 == 1 {
        Ok(())
    } else {
        Err(AppError::NotFound("Review project not found".into()))
    }
}

async fn load(
    conn: &mut SqliteConnection,
    user_id: &str,
    review_id: Uuid,
) -> AppResult<ReviewSession> {
    let payload: Option<(String,)> = sqlx::query_as("SELECT r.payload FROM review_sessions r JOIN projects p ON p.id = r.project_id WHERE r.id = ? AND p.user_id = ?")
        .bind(review_id.to_string()).bind(user_id).fetch_optional(&mut *conn).await?;
    let mut session: ReviewSession = decode(
        &payload
            .ok_or_else(|| AppError::NotFound("Review not found".into()))?
            .0,
    )?;
    let snapshot: Option<(String,)> =
        sqlx::query_as("SELECT payload FROM review_snapshots WHERE review_id = ?")
            .bind(review_id.to_string())
            .fetch_optional(&mut *conn)
            .await?;
    session.snapshot = snapshot.map(|s| decode(&s.0)).transpose()?;
    let steps: Vec<(String,)> =
        sqlx::query_as("SELECT payload FROM review_steps WHERE review_id = ? ORDER BY rowid")
            .bind(review_id.to_string())
            .fetch_all(&mut *conn)
            .await?;
    session.steps = steps
        .into_iter()
        .map(|s| decode(&s.0))
        .collect::<AppResult<_>>()?;
    let findings: Vec<(String,)> =
        sqlx::query_as("SELECT payload FROM review_findings WHERE review_id = ? ORDER BY rowid")
            .bind(review_id.to_string())
            .fetch_all(&mut *conn)
            .await?;
    session.findings = findings
        .into_iter()
        .map(|s| decode(&s.0))
        .collect::<AppResult<_>>()?;
    session.validate()?;
    Ok(session)
}

/// Create a queued review and pin non-secret provider/model/prompt provenance.
/// # Errors
/// Rejects foreign projects/configs or malformed provenance. Active selection is validated by the future start service.
pub async fn create(
    pool: &SqlitePool,
    user_id: &str,
    project_id: Uuid,
    pinned: ReviewProvenance,
) -> AppResult<ReviewSession> {
    provenance(&pinned)?;
    let mut tx = pool.begin().await?;
    owned(&mut tx, user_id, project_id).await?;
    let config: Option<(String,)> =
        sqlx::query_as("SELECT provider FROM user_provider_configs WHERE id = ? AND user_id = ?")
            .bind(pinned.provider_config_id.to_string())
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    ensure(
        config.is_some_and(|c| encode(&pinned.provider).is_ok_and(|s| s == format!("\"{}\"", c.0))),
    )?;
    let now = Utc::now().to_rfc3339();
    let session = ReviewSession {
        review_id: Uuid::new_v4(),
        project_id,
        scope: ReviewScope::Staged,
        status: ReviewStatus::Queued,
        sequence: 0,
        progress: 0,
        provenance: pinned,
        created_at: now.clone(),
        updated_at: now.clone(),
        outcome: None,
        error: None,
        snapshot: None,
        steps: vec![],
        findings: vec![],
        truncation: vec![],
    };
    sqlx::query("INSERT INTO review_sessions(id,project_id,status,sequence,payload,created_at,updated_at) VALUES (?,?,'queued',0,?,?,?)")
        .bind(session.review_id.to_string()).bind(project_id.to_string()).bind(encode(&session)?).bind(&now).bind(&now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(session)
}

/// Fetch an authoritative review. Caller identity is required even for known UUIDs.
/// # Errors
/// Returns not found for missing/foreign reviews and invalid input for corrupt DTOs.
pub async fn fetch(pool: &SqlitePool, user_id: &str, review_id: Uuid) -> AppResult<ReviewSession> {
    let mut tx = pool.begin().await?;
    let session = load(&mut tx, user_id, review_id).await?;
    tx.commit().await?;
    Ok(session)
}

/// List newest reviews for an owned project, bounded to 200 records.
/// # Errors
/// Returns not found for foreign projects or database/decode errors.
pub async fn list(
    pool: &SqlitePool,
    user_id: &str,
    project_id: Uuid,
    limit: u32,
) -> AppResult<Vec<ReviewSession>> {
    let mut tx = pool.begin().await?;
    owned(&mut tx, user_id, project_id).await?;
    let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM review_sessions WHERE project_id = ? ORDER BY created_at DESC, id DESC LIMIT ?")
        .bind(project_id.to_string()).bind(limit.clamp(1, 200)).fetch_all(&mut *tx).await?;
    let mut result = Vec::with_capacity(ids.len());
    for (id,) in ids {
        result.push(
            load(
                &mut tx,
                user_id,
                Uuid::parse_str(&id).map_err(|_| invalid())?,
            )
            .await?,
        );
    }
    tx.commit().await?;
    Ok(result)
}

/// Attach one immutable snapshot plus bytes in manifest order. No source appears in the DTO.
/// # Errors
/// Rejects terminal sessions, repeated capture, bad hashes/lengths, or malformed manifests; rolls back all inserts.
pub async fn attach_snapshot(
    pool: &SqlitePool,
    user_id: &str,
    review_id: Uuid,
    snapshot: &ReviewSnapshot,
    source_bytes: &[Vec<u8>],
) -> AppResult<()> {
    snapshot.validate()?;
    ensure(source_bytes.len() == snapshot.sources.len())?;
    let mut tx = pool.begin().await?;
    let session = load(&mut tx, user_id, review_id).await?;
    ensure(!session.status.is_terminal() && session.snapshot.is_none())?;
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO review_snapshots(id,review_id,payload,created_at,updated_at) VALUES (?,?,?,?,?)")
        .bind(snapshot.snapshot_id.to_string()).bind(review_id.to_string()).bind(encode(snapshot)?).bind(&now).bind(&now).execute(&mut *tx).await?;
    for (source, bytes) in snapshot.sources.iter().zip(source_bytes) {
        ensure(
            digest(bytes) == source.content_hash
                && bytes.len() == source.byte_length as usize
                && lines(bytes) == source.line_count,
        )?;
        sqlx::query("INSERT INTO review_sources(id,snapshot_id,side,path,content,created_at,updated_at) VALUES (?,?,?,?,?,?,?)")
            .bind(Uuid::new_v4().to_string()).bind(snapshot.snapshot_id.to_string())
            .bind(if source.side == SourceSide::Old { "old" } else { "new" }).bind(&source.path).bind(bytes).bind(&now).bind(&now).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Retrieve exact raw bytes from stored capture (for redaction/materialization later).
/// # Errors
/// Returns not found for foreign reviews or invalid input for unbound source references.
pub async fn fetch_source(
    pool: &SqlitePool,
    user_id: &str,
    review_id: Uuid,
    reference: &ReviewSourceReference,
) -> AppResult<Vec<u8>> {
    let mut tx = pool.begin().await?;
    let session = load(&mut tx, user_id, review_id).await?;
    reference.validate(session.snapshot.as_ref().ok_or_else(invalid)?)?;
    let row: (Vec<u8>,) = sqlx::query_as(
        "SELECT content FROM review_sources WHERE snapshot_id = ? AND side = ? AND path = ?",
    )
    .bind(reference.snapshot_id.to_string())
    .bind(if reference.side == SourceSide::Old {
        "old"
    } else {
        "new"
    })
    .bind(&reference.path)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.0)
}

/// Compare-and-set progress/final results. New steps/findings append; all pinned identity remains unchanged.
/// The returned sequence is persisted before callers emit progress hints.
/// # Errors
/// Rejects stale sequence, terminal updates, invalid transitions, and modified history. All child inserts roll back on error.
pub async fn update(
    pool: &SqlitePool,
    user_id: &str,
    expected_sequence: u32,
    mut next: ReviewSession,
) -> AppResult<ReviewSession> {
    next.validate()?;
    let mut tx = pool.begin().await?;
    let old = load(&mut tx, user_id, next.review_id).await?;
    ensure(
        old.sequence == expected_sequence
            && next.sequence == old.sequence
            && old.status.can_transition(next.status)
            && old.project_id == next.project_id
            && old.scope == next.scope
            && old.created_at == next.created_at
            && old.snapshot == next.snapshot
            && old.progress <= next.progress
            && old.provenance.provider_config_id == next.provenance.provider_config_id
            && old.provenance.provider == next.provenance.provider
            && old.provenance.model == next.provenance.model
            && old.provenance.prompt_version == next.provenance.prompt_version
            && next.steps.starts_with(&old.steps)
            && next.findings.starts_with(&old.findings)
            && next.truncation.starts_with(&old.truncation)
            && next
                .findings
                .iter()
                .all(|f| f.evidence_state == EvidenceState::Suspected),
    )?;
    next.sequence = old.sequence.checked_add(1).ok_or_else(invalid)?;
    next.updated_at = Utc::now().to_rfc3339();
    for step in &next.steps[old.steps.len()..] {
        sqlx::query("INSERT INTO review_steps(id,review_id,payload,created_at,updated_at) VALUES (?,?,?,?,?)")
            .bind(step.step_id.to_string()).bind(next.review_id.to_string()).bind(encode(step)?).bind(&step.created_at).bind(&step.created_at).execute(&mut *tx).await?;
    }
    for finding in &next.findings[old.findings.len()..] {
        sqlx::query("INSERT INTO review_findings(id,review_id,payload,created_at,updated_at) VALUES (?,?,?,?,?)")
            .bind(finding.finding_id.to_string()).bind(next.review_id.to_string()).bind(encode(finding)?).bind(&next.updated_at).bind(&next.updated_at).execute(&mut *tx).await?;
    }
    let mut header = next.clone();
    header.snapshot = None;
    header.steps.clear();
    header.findings.clear();
    let status = encode(&next.status)?;
    let changed = sqlx::query("UPDATE review_sessions SET status=?, sequence=?, payload=?, updated_at=? WHERE id=? AND sequence=? AND status IN ('queued','running')")
        .bind(status.trim_matches('"')).bind(next.sequence).bind(encode(&header)?).bind(&next.updated_at).bind(next.review_id.to_string()).bind(expected_sequence).execute(&mut *tx).await?;
    ensure(changed.rows_affected() == 1)?;
    tx.commit().await?;
    Ok(next)
}

/// Called once after migrations during pool startup; never resumes orphaned work.
/// # Errors
/// Returns database errors or sequence overflow, rolling back recovery as a batch.
pub async fn interrupt_active(pool: &SqlitePool) -> AppResult<u64> {
    let mut tx = pool.begin().await?;
    let overflow: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM review_sessions WHERE status IN ('queued','running') AND sequence >= ?",
    )
    .bind(i64::from(u32::MAX))
    .fetch_one(&mut *tx)
    .await?;
    ensure(overflow.0 == 0)?;
    let now = Utc::now().to_rfc3339();
    let error = ReviewError {
        code: "INTERRUPTED".into(),
        message: "Review interrupted by application restart; start a new review".into(),
    };
    // Recover lifecycle without decoding the whole DTO: a malformed/older payload
    // must not prevent application startup. Fetch still validates it fail-closed.
    let changed = sqlx::query("UPDATE review_sessions SET status='interrupted', sequence=sequence+1, updated_at=?, payload=json_set(payload, '$.status', 'interrupted', '$.sequence', sequence+1, '$.updatedAt', ?, '$.error', json(?)) WHERE status IN ('queued','running')")
        .bind(&now).bind(&now).bind(encode(&error)?).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(changed.rows_affected())
}

/// Link a test artifact/version to the exact finding/snapshot and generating provenance.
/// # Errors
/// Rejects unknown/foreign IDs, mismatched artifact version/content, and incomplete reviews.
pub async fn link_artifact(
    pool: &SqlitePool,
    user_id: &str,
    review_id: Uuid,
    link: &ReviewArtifactLink,
) -> AppResult<()> {
    provenance(&link.provenance)?;
    ensure(link.test_version > 0 && validation::hash(&link.test_content_hash, 64))?;
    let mut tx = pool.begin().await?;
    let session = load(&mut tx, user_id, review_id).await?;
    ensure(
        session.status == ReviewStatus::Completed
            && session
                .snapshot
                .as_ref()
                .is_some_and(|s| s.snapshot_id == link.snapshot_id)
            && session
                .findings
                .iter()
                .any(|f| f.finding_id == link.finding_id),
    )?;
    let artifact: Option<(String, i64)> =
        sqlx::query_as("SELECT content_md,version FROM artifacts WHERE id=? AND project_id=?")
            .bind(link.artifact_id.to_string())
            .bind(session.project_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
    let (content, version) = artifact.ok_or_else(invalid)?;
    ensure(
        version == i64::from(link.test_version)
            && digest(content.as_bytes()) == link.test_content_hash,
    )?;
    let config: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM user_provider_configs WHERE id=? AND user_id=? AND provider=?",
    )
    .bind(link.provenance.provider_config_id.to_string())
    .bind(user_id)
    .bind(encode(&link.provenance.provider)?.trim_matches('"'))
    .fetch_one(&mut *tx)
    .await?;
    ensure(config.0 == 1)?;
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO review_artifact_links(id,finding_id,artifact_id,payload,created_at,updated_at) VALUES (?,?,?,?,?,?)")
        .bind(link.link_id.to_string()).bind(link.finding_id.to_string()).bind(link.artifact_id.to_string()).bind(encode(link)?).bind(&now).bind(&now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Append an evidence attempt without modifying a completed finding or earlier attempts.
/// A failing process alone is never reproduction: the identified failed assertion must match an existing case.
/// # Errors
/// Rejects unknown/foreign run bindings or insufficient reproduction evidence.
pub async fn link_run(
    pool: &SqlitePool,
    user_id: &str,
    review_id: Uuid,
    link: &ReviewRunLink,
) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    let session = load(&mut tx, user_id, review_id).await?;
    ensure(
        session.status == ReviewStatus::Completed
            && link.evidence_state != EvidenceState::Suspected,
    )?;
    let artifact: Option<(String,)> = sqlx::query_as("SELECT l.payload FROM review_artifact_links l JOIN review_findings f ON f.id=l.finding_id WHERE l.id=? AND f.review_id=?")
        .bind(link.artifact_link_id.to_string()).bind(review_id.to_string()).fetch_optional(&mut *tx).await?;
    let artifact: ReviewArtifactLink = decode(&artifact.ok_or_else(invalid)?.0)?;
    let run: Option<RunBindingRow> = sqlx::query_as("SELECT artifact_id,project_id,status,runner,started_at,finished_at FROM test_runs WHERE id=?")
        .bind(link.run_id.to_string()).fetch_optional(&mut *tx).await?;
    let (artifact_id, project_id, status, runner, started, finished) = run.ok_or_else(invalid)?;
    let env = &link.environment;
    ensure(
        text(&env.runner)
            && env
                .image_id
                .strip_prefix("sha256:")
                .is_some_and(|s| validation::hash(s, 64))
            && validation::timestamp(&env.started_at)
            && validation::timestamp(&env.finished_at)
            && env.timeout_ms > 0
            && env.memory_bytes > 0
            && env.cpu_millis > 0
            && runner == env.runner
            && started.as_deref() == Some(&env.started_at)
            && finished.as_deref() == Some(&env.finished_at),
    )?;
    for detail in [&link.assertion, &link.expected, &link.observed]
        .into_iter()
        .flatten()
    {
        ensure(text(detail))?;
    }
    ensure(
        artifact_id == artifact.artifact_id.to_string()
            && project_id == session.project_id.to_string()
            && matches!(status.as_str(), "passed" | "failed" | "error" | "cancelled"),
    )?;
    if link.evidence_state == EvidenceState::Reproduced {
        ensure(
            status == "failed"
                && !env.output_truncated
                && link.assertion.as_deref().is_some_and(text)
                && link.expected.as_deref().is_some_and(text)
                && link.observed.as_deref().is_some_and(text),
        )?;
        let case: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM test_run_cases WHERE run_id=? AND name=? AND status='failed'",
        )
        .bind(link.run_id.to_string())
        .bind(&link.assertion)
        .fetch_one(&mut *tx)
        .await?;
        ensure(case.0 > 0)?;
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO review_run_links(id,artifact_link_id,run_id,payload,created_at,updated_at) VALUES (?,?,?,?,?,?)")
        .bind(link.attempt_id.to_string()).bind(link.artifact_link_id.to_string()).bind(link.run_id.to_string()).bind(encode(link)?).bind(&now).bind(&now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Fetch append-only test bindings and evidence attempts separately from immutable review output.
/// # Errors
/// Returns not found for foreign reviews, or database/decode errors.
pub async fn fetch_links(
    pool: &SqlitePool,
    user_id: &str,
    review_id: Uuid,
) -> AppResult<(Vec<ReviewArtifactLink>, Vec<ReviewRunLink>)> {
    let mut tx = pool.begin().await?;
    load(&mut tx, user_id, review_id).await?;
    let artifacts: Vec<(String,)> = sqlx::query_as("SELECT l.payload FROM review_artifact_links l JOIN review_findings f ON f.id=l.finding_id WHERE f.review_id=? ORDER BY l.rowid")
        .bind(review_id.to_string()).fetch_all(&mut *tx).await?;
    let runs: Vec<(String,)> = sqlx::query_as("SELECT r.payload FROM review_run_links r JOIN review_artifact_links l ON l.id=r.artifact_link_id JOIN review_findings f ON f.id=l.finding_id WHERE f.review_id=? ORDER BY r.rowid")
        .bind(review_id.to_string()).fetch_all(&mut *tx).await?;
    let result = (
        artifacts
            .into_iter()
            .map(|r| decode(&r.0))
            .collect::<AppResult<_>>()?,
        runs.into_iter()
            .map(|r| decode(&r.0))
            .collect::<AppResult<_>>()?,
    );
    tx.commit().await?;
    Ok(result)
}

#[cfg(test)]
mod tests;
