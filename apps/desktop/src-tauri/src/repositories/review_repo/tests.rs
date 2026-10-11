use super::*;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;

const USER: &str = "00000000-0000-4000-8000-000000000001";
const FIXTURE: &str =
    include_str!("../../../../../../packages/shared/src/schemas/fixtures/review.json");
const INVALID: &str =
    include_str!("../../../../../../packages/shared/src/schemas/fixtures/review-invalid.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}
fn session_fixture() -> ReviewSession {
    serde_json::from_value(fixture()["session"].clone()).unwrap()
}

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    let template = session_fixture();
    let now = &template.created_at;
    sqlx::query("INSERT INTO projects(id,user_id,name,root_path,created_at,updated_at) VALUES (?,?,'review','/tmp',?,?)")
        .bind(template.project_id.to_string()).bind(USER).bind(now).bind(now).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO user_provider_configs(id,user_id,provider,is_active,created_at,updated_at) VALUES (?,?,'ollama',1,?,?)")
        .bind(template.provenance.provider_config_id.to_string()).bind(USER).bind(now).bind(now).execute(&pool).await.unwrap();
    pool
}

async fn queued(pool: &SqlitePool) -> ReviewSession {
    let fixture = session_fixture();
    create(pool, USER, fixture.project_id, fixture.provenance)
        .await
        .unwrap()
}

async fn completed(pool: &SqlitePool) -> ReviewSession {
    let mut review = queued(pool).await;
    review.status = ReviewStatus::Running;
    review = update(pool, USER, 0, review).await.unwrap();
    let template = session_fixture();
    let snapshot = template.snapshot.unwrap();
    attach_snapshot(
        pool,
        USER,
        review.review_id,
        &snapshot,
        &[fixture()["sourceUtf8"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec()],
    )
    .await
    .unwrap();
    review = fetch(pool, USER, review.review_id).await.unwrap();
    review.status = ReviewStatus::Completed;
    review.progress = 100;
    review.outcome = Some(ReviewOutcome::Reviewed);
    review.findings = template.findings;
    review.steps = template.steps;
    review.truncation = template.truncation;
    update(pool, USER, 1, review).await.unwrap()
}

#[test]
fn shared_wire_round_trip_and_malformed_corpus() {
    let review = session_fixture();
    review.validate().unwrap();
    assert_eq!(serde_json::to_value(review).unwrap(), fixture()["session"]);
    let artifact: ReviewArtifactLink =
        serde_json::from_value(fixture()["artifactLink"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(artifact).unwrap(),
        fixture()["artifactLink"]
    );
    let run: ReviewRunLink = serde_json::from_value(fixture()["runLink"].clone()).unwrap();
    assert_eq!(serde_json::to_value(run).unwrap(), fixture()["runLink"]);
    let mutations: Vec<Value> = serde_json::from_str(INVALID).unwrap();
    for mutation in mutations {
        let mut value = fixture()["session"].clone();
        let parts: Vec<_> = mutation["path"].as_str().unwrap().split('.').collect();
        let mut target = &mut value;
        for part in &parts[..parts.len() - 1] {
            target = if let Ok(index) = part.parse::<usize>() {
                &mut target[index]
            } else {
                &mut target[*part]
            };
        }
        target[parts[parts.len() - 1]] = mutation["value"].clone();
        assert!(
            serde_json::from_value::<ReviewSession>(value)
                .and_then(|r| r.validate().map_err(serde::de::Error::custom))
                .is_err(),
            "mutation {} accepted",
            mutation["path"]
        );
    }
    for key in [
        "reviewId",
        "projectId",
        "provenance",
        "steps",
        "findings",
        "createdAt",
    ] {
        let mut value = fixture()["session"].clone();
        value.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<ReviewSession>(value).is_err());
    }
}

#[tokio::test]
async fn create_fetch_list_ownership_and_terminal_guards() {
    let pool = pool().await;
    let review = completed(&pool).await;
    assert_eq!(fetch(&pool, USER, review.review_id).await.unwrap(), review);
    assert_eq!(
        list(&pool, USER, review.project_id, 20).await.unwrap(),
        vec![review.clone()]
    );
    assert!(fetch(&pool, "foreign-user", review.review_id)
        .await
        .is_err());
    assert!(list(&pool, "foreign-user", review.project_id, 20)
        .await
        .is_err());
    assert!(create(
        &pool,
        "foreign-user",
        review.project_id,
        review.provenance.clone()
    )
    .await
    .is_err());
    assert!(fetch(&pool, USER, Uuid::new_v4()).await.is_err());
    let mut late = review.clone();
    late.status = ReviewStatus::Running;
    late.outcome = None;
    assert!(update(&pool, USER, review.sequence, late).await.is_err());
    assert!(update(&pool, USER, review.sequence, review.clone())
        .await
        .is_err());
    assert!(
        sqlx::query("UPDATE review_sessions SET status='running' WHERE id=?")
            .bind(review.review_id.to_string())
            .execute(&pool)
            .await
            .is_err()
    );
    let reference = &review.findings[0].sources[0];
    assert_eq!(
        fetch_source(&pool, USER, review.review_id, reference)
            .await
            .unwrap(),
        fixture()["sourceUtf8"].as_str().unwrap().as_bytes()
    );
    assert!(
        fetch_source(&pool, "foreign-user", review.review_id, reference)
            .await
            .is_err()
    );
    assert!(attach_snapshot(
        &pool,
        USER,
        review.review_id,
        review.snapshot.as_ref().unwrap(),
        &[]
    )
    .await
    .is_err());
}

#[tokio::test]
async fn transitions_stale_events_and_history_are_atomic() {
    let pool = pool().await;
    let mut review = queued(&pool).await;
    let mut invalid_complete = review.clone();
    invalid_complete.status = ReviewStatus::Completed;
    invalid_complete.progress = 100;
    invalid_complete.outcome = Some(ReviewOutcome::NoChanges);
    invalid_complete.snapshot = session_fixture().snapshot;
    assert!(update(&pool, USER, 0, invalid_complete).await.is_err());
    review.status = ReviewStatus::Running;
    review.steps = session_fixture().steps;
    review = update(&pool, USER, 0, review).await.unwrap();
    assert!(update(&pool, USER, 0, review.clone()).await.is_err());
    let mut omitted = review.clone();
    omitted.truncation = session_fixture().truncation;
    review = update(&pool, USER, 1, omitted).await.unwrap();
    let mut removed = review.clone();
    removed.truncation.clear();
    assert!(update(&pool, USER, 2, removed).await.is_err());
    let mut overwritten = review.clone();
    overwritten.steps[0].progress = 0;
    assert!(update(&pool, USER, 2, overwritten).await.is_err());
    // Duplicate global step ID fails after inserting a different step in this transaction.
    let mut next = review.clone();
    let mut fresh = next.steps[0].clone();
    fresh.step_id = Uuid::new_v4();
    next.steps.push(fresh.clone());
    let other = queued(&pool).await;
    let mut colliding = fresh;
    colliding.step_id = Uuid::new_v4();
    let mut other_update = other;
    other_update.steps.push(colliding.clone());
    update(&pool, USER, 0, other_update).await.unwrap();
    next.steps.push(colliding);
    assert!(update(&pool, USER, 2, next).await.is_err());
    assert_eq!(fetch(&pool, USER, review.review_id).await.unwrap(), review);
    review.status = ReviewStatus::Cancelled;
    let cancelled = update(&pool, USER, 2, review).await.unwrap();
    assert!(update(&pool, USER, 3, cancelled).await.is_err());
}

#[tokio::test]
async fn capture_hash_validation_rolls_back_and_preserves_source() {
    let pool = pool().await;
    let review = queued(&pool).await;
    let snapshot = session_fixture().snapshot.unwrap();
    assert!(attach_snapshot(
        &pool,
        USER,
        review.review_id,
        &snapshot,
        &[b"wrong".to_vec()]
    )
    .await
    .is_err());
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM review_snapshots")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0);
    let bytes = fixture()["sourceUtf8"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    attach_snapshot(
        &pool,
        USER,
        review.review_id,
        &snapshot,
        std::slice::from_ref(&bytes),
    )
    .await
    .unwrap();
    assert!(
        attach_snapshot(&pool, USER, review.review_id, &snapshot, &[bytes])
            .await
            .is_err()
    );
    assert!(sqlx::query("UPDATE review_sources SET content=x'00'")
        .execute(&pool)
        .await
        .is_err());
    assert!(sqlx::query("UPDATE review_snapshots SET payload='{}'")
        .execute(&pool)
        .await
        .is_err());
    let mut bad_ref = session_fixture().findings[0].sources[0].clone();
    bad_ref.line_end = Some(2);
    assert!(fetch_source(&pool, USER, review.review_id, &bad_ref)
        .await
        .is_err());
}

#[tokio::test]
async fn startup_marks_orphans_interrupted_without_touching_terminal_results() {
    let pool = pool().await;
    let terminal = completed(&pool).await;
    let queued = queued(&pool).await;
    let mut running = super::tests::queued(&pool).await;
    running.status = ReviewStatus::Running;
    running = update(&pool, USER, 0, running).await.unwrap();
    assert_eq!(interrupt_active(&pool).await.unwrap(), 2);
    assert_eq!(interrupt_active(&pool).await.unwrap(), 0);
    for id in [queued.review_id, running.review_id] {
        let orphan = fetch(&pool, USER, id).await.unwrap();
        assert_eq!(orphan.status, ReviewStatus::Interrupted);
        assert_eq!(orphan.error.unwrap().code, "INTERRUPTED");
    }
    assert_eq!(
        fetch(&pool, USER, terminal.review_id).await.unwrap(),
        terminal
    );
}

#[tokio::test]
async fn recovery_does_not_decode_corrupt_headers_or_hide_sequence_overflow() {
    let pool = pool().await;
    let review = queued(&pool).await;
    sqlx::query("UPDATE review_sessions SET payload='{}', sequence=? WHERE id=?")
        .bind(i64::from(u32::MAX))
        .bind(review.review_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    assert!(interrupt_active(&pool).await.is_err());
    let status: (String,) = sqlx::query_as("SELECT status FROM review_sessions WHERE id=?")
        .bind(review.review_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status.0, "queued");
    sqlx::query("UPDATE review_sessions SET sequence=0 WHERE id=?")
        .bind(review.review_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    let valid = queued(&pool).await;
    assert_eq!(interrupt_active(&pool).await.unwrap(), 2);
    assert_eq!(
        fetch(&pool, USER, valid.review_id).await.unwrap().status,
        ReviewStatus::Interrupted
    );
    // Recovery retains the malformed payload rather than inventing missing identity.
    assert!(fetch(&pool, USER, review.review_id).await.is_err());
    let row: (String, i64, String) =
        sqlx::query_as("SELECT status,sequence,payload FROM review_sessions WHERE id=?")
            .bind(review.review_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "interrupted");
    assert_eq!(row.1, 1);
    assert_eq!(
        serde_json::from_str::<Value>(&row.2).unwrap()["error"]["code"],
        "INTERRUPTED"
    );
}

#[tokio::test]
async fn upgrade_old_database_is_additive_and_idempotent() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    // Install only the original ten migrations using their actual checksums/versions.
    let all = sqlx::migrate!("./migrations");
    let older = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter().filter(|m| m.version <= 10).cloned().collect(),
        ),
        ..all
    };
    older.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO projects(id,user_id,name,root_path,created_at,updated_at) VALUES ('old',?,'keep','/tmp','now','now')").bind(USER).execute(&pool).await.unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    let name: (String,) = sqlx::query_as("SELECT name FROM projects WHERE id='old'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name.0, "keep");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 11);
}

#[tokio::test]
async fn evidence_bindings_reject_unknown_ids_and_keep_attempts_append_only() {
    let pool = pool().await;
    let review = completed(&pool).await;
    let artifact: ReviewArtifactLink =
        serde_json::from_value(fixture()["artifactLink"].clone()).unwrap();
    assert!(link_artifact(&pool, USER, review.review_id, &artifact)
        .await
        .is_err());
    let stamp = &review.created_at;
    sqlx::query("INSERT INTO artifacts(id,project_id,artifact_type,title,content_md,version,created_at,updated_at) VALUES (?,?,'test_cases','test',?,1,?,?)")
        .bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(fixture()["artifactContent"].as_str().unwrap()).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    let mut bad = artifact.clone();
    bad.test_version = 2;
    assert!(link_artifact(&pool, USER, review.review_id, &bad)
        .await
        .is_err());
    bad = artifact.clone();
    bad.test_content_hash = "0".repeat(64);
    assert!(link_artifact(&pool, USER, review.review_id, &bad)
        .await
        .is_err());
    bad = artifact.clone();
    bad.finding_id = Uuid::new_v4();
    assert!(link_artifact(&pool, USER, review.review_id, &bad)
        .await
        .is_err());
    assert!(
        link_artifact(&pool, "foreign-user", review.review_id, &artifact)
            .await
            .is_err()
    );
    link_artifact(&pool, USER, review.review_id, &artifact)
        .await
        .unwrap();
    let run: ReviewRunLink = serde_json::from_value(fixture()["runLink"].clone()).unwrap();
    assert!(link_run(&pool, USER, review.review_id, &run).await.is_err());
    let env = &run.environment;
    sqlx::query("INSERT INTO test_runs(id,artifact_id,project_id,status,runner,started_at,finished_at,created_at,updated_at) VALUES (?,?,?,'failed',?,?,?,?,?)")
        .bind(run.run_id.to_string()).bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(&env.runner).bind(&env.started_at).bind(&env.finished_at).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    // A failed process without the cited assertion is inconclusive, never reproduced.
    assert!(link_run(&pool, USER, review.review_id, &run).await.is_err());
    sqlx::query("INSERT INTO test_run_cases(id,run_id,name,status,created_at,updated_at) VALUES (?, ?,?,'failed',?,?)")
        .bind(Uuid::new_v4().to_string()).bind(run.run_id.to_string()).bind(&run.assertion).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    let mut clipped = run.clone();
    clipped.environment.output_truncated = true;
    assert!(link_run(&pool, USER, review.review_id, &clipped)
        .await
        .is_err());
    link_run(&pool, USER, review.review_id, &run).await.unwrap();
    assert!(link_run(&pool, USER, review.review_id, &run).await.is_err());
    assert!(sqlx::query("UPDATE review_run_links SET payload='{}'")
        .execute(&pool)
        .await
        .is_err());
    let mut inconclusive = run.clone();
    inconclusive.attempt_id = Uuid::new_v4();
    inconclusive.run_id = Uuid::new_v4();
    inconclusive.evidence_state = EvidenceState::Inconclusive;
    sqlx::query("INSERT INTO test_runs(id,artifact_id,project_id,status,runner,started_at,finished_at,created_at,updated_at) VALUES (?,?,?,'passed',?,?,?,?,?)")
        .bind(inconclusive.run_id.to_string()).bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(&env.runner).bind(&env.started_at).bind(&env.finished_at).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    link_run(&pool, USER, review.review_id, &inconclusive)
        .await
        .unwrap();
    let (artifacts, attempts) = fetch_links(&pool, USER, review.review_id).await.unwrap();
    assert_eq!(artifacts, vec![artifact]);
    assert_eq!(attempts, vec![run, inconclusive]);
    assert_eq!(fetch(&pool, USER, review.review_id).await.unwrap(), review);
    // Project deletion can clean its own descendants despite provenance retention FKs.
    sqlx::query("DELETE FROM projects WHERE id=?")
        .bind(review.project_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
}
