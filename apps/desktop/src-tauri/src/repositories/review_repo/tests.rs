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
    seed_pool(&pool).await;
    pool
}

async fn seed_pool(pool: &SqlitePool) {
    let template = session_fixture();
    let now = &template.created_at;
    sqlx::query("INSERT INTO projects(id,user_id,name,root_path,created_at,updated_at) VALUES (?,?,'review','/tmp',?,?)")
        .bind(template.project_id.to_string()).bind(USER).bind(now).bind(now).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO user_provider_configs(id,user_id,provider,is_active,created_at,updated_at) VALUES (?,?,'ollama',1,?,?)")
        .bind(template.provenance.provider_config_id.to_string()).bind(USER).bind(now).bind(now).execute(pool).await.unwrap();
}

async fn remove_test_directory(path: &std::path::Path) {
    // SQLite pool returns can finish closing worker handles asynchronously.
    // Windows refuses deletion during that short interval. Retry only sharing
    // violations; persistent handles and every other cleanup error still fail.
    for _ in 0..20 {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return,
            Err(error) if cfg!(windows) && error.raw_os_error() == Some(32) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(error) => panic!("test directory cleanup failed: {error}"),
        }
    }
    std::fs::remove_dir_all(path).unwrap();
}

async fn queued(pool: &SqlitePool) -> ReviewSession {
    let fixture = session_fixture();
    create(pool, USER, fixture.project_id, fixture.provenance)
        .await
        .unwrap()
}

async fn completed(pool: &SqlitePool) -> ReviewSession {
    completed_from(
        pool,
        session_fixture(),
        fixture()["sourceUtf8"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec(),
    )
    .await
}

async fn completed_from(
    pool: &SqlitePool,
    mut template: ReviewSession,
    bytes: Vec<u8>,
) -> ReviewSession {
    let mut review = queued(pool).await;
    review.status = ReviewStatus::Running;
    review = update(pool, USER, 0, review).await.unwrap();
    let snapshot = template.snapshot.take().unwrap();
    attach_snapshot(
        pool,
        USER,
        review.review_id,
        &snapshot,
        std::slice::from_ref(&bytes),
    )
    .await
    .unwrap();
    review = fetch(pool, USER, review.review_id).await.unwrap();
    review.status = ReviewStatus::Completed;
    review.progress = 100;
    review.outcome = Some(ReviewOutcome::Reviewed);
    review.findings = template.findings;
    review.excluded_files = template.excluded_files;
    review.steps = template.steps;
    review.truncation = template.truncation;
    update(pool, USER, 1, review).await.unwrap()
}

#[tokio::test]
async fn persists_suggested_tests_and_final_omissions_without_changing_snapshot() {
    let pool = pool().await;
    let mut template = session_fixture();
    template.findings[0].suggested_test = Some("Assert rejection of negative quantities".into());
    template.excluded_files = Some(vec![ReviewExclusion {
        path: "src/support.ts".into(),
        reason: "prompt_budget".into(),
    }]);
    template.truncation.push(ReviewTruncation {
        resource: "final_prompt".into(),
        reason: "prompt_budget".into(),
        omitted_count: 1,
    });
    let snapshot = template.snapshot.clone();
    let review = completed_from(
        &pool,
        template,
        fixture()["sourceUtf8"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec(),
    )
    .await;
    let saved = fetch(&pool, USER, review.review_id).await.unwrap();
    assert_eq!(saved, review);
    assert_eq!(saved.snapshot, snapshot);
    assert_eq!(
        saved.findings[0].suggested_test.as_deref(),
        Some("Assert rejection of negative quantities")
    );
    assert_eq!(saved.excluded_files.unwrap()[0].reason, "prompt_budget");
    assert!(saved
        .truncation
        .iter()
        .any(|t| t.resource == "final_prompt"));
}

#[test]
fn optional_suggested_tests_and_omissions_match_zod_without_changing_legacy_payloads() {
    let original = fixture()["session"].clone();
    assert_eq!(serde_json::to_value(session_fixture()).unwrap(), original);
    for value in [
        serde_json::json!(""),
        serde_json::json!("   "),
        Value::Null,
        serde_json::json!("x".repeat(16385)),
        serde_json::json!("\0"),
    ] {
        let mut candidate = original.clone();
        candidate["findings"][0]["suggestedTest"] = value;
        assert!(
            !serde_json::from_value::<ReviewSession>(candidate).is_ok_and(|r| r.validate().is_ok())
        );
    }
    for value in [
        Value::Null,
        serde_json::json!([{"path":"src/a.ts","reason":""}]),
    ] {
        let mut candidate = original.clone();
        candidate["excludedFiles"] = value;
        assert!(
            !serde_json::from_value::<ReviewSession>(candidate).is_ok_and(|r| r.validate().is_ok())
        );
    }
}

#[tokio::test]
async fn final_omissions_are_append_only() {
    let pool = pool().await;
    let mut review = queued(&pool).await;
    review.status = ReviewStatus::Running;
    review.excluded_files = Some(vec![ReviewExclusion {
        path: "src/a.ts".into(),
        reason: "redaction_unmappable".into(),
    }]);
    let review = update(&pool, USER, 0, review).await.unwrap();
    let mut rewritten = review.clone();
    rewritten.excluded_files = None;
    assert!(update(&pool, USER, 1, rewritten).await.is_err());
    assert_eq!(fetch(&pool, USER, review.review_id).await.unwrap(), review);
}

fn mode_snapshot(count: usize) -> ReviewSnapshot {
    let mut snapshot = session_fixture().snapshot.unwrap();
    let source = snapshot.sources[0].clone();
    snapshot.sources.clear();
    snapshot.changes.clear();
    for i in 0..count {
        let path = format!("src/file{i}.ts");
        let mut old = source.clone();
        old.path = path.clone();
        old.side = SourceSide::Old;
        old.blob_id = format!("{i:040x}");
        old.content_hash = format!("{i:064x}");
        old.byte_length = 120 * 1024;
        let mut new = old.clone();
        new.side = SourceSide::New;
        new.mode = "100755".into();
        snapshot.sources.extend([old, new]);
        snapshot.changes.push(ReviewChange {
            change_kind: ReviewChangeKind::Modified,
            old_path: Some(path.clone()),
            new_path: Some(path),
        });
    }
    snapshot
}

#[test]
fn saved_test_hash_covers_sorted_exact_file_bytes() {
    let files = serde_json::json!({"files": [
        {"path": "z.test.ts", "contents": "assert(false);\r\n\n", "isTest": true},
        {"path": "src/é.ts", "contents": "export const value = 1;\n"}
    ]});
    // Independently specified serialized representation: no Markdown or trimming.
    let representation = "[[\"src/é.ts\",\"export const value = 1;\\n\"],[\"z.test.ts\",\"assert(false);\\r\\n\\n\"]]";
    let expected = digest(representation.as_bytes());
    assert_eq!(saved_test_content_hash(&files).unwrap(), expected);
    let mut reordered = files.clone();
    reordered["files"].as_array_mut().unwrap().reverse();
    assert_eq!(saved_test_content_hash(&reordered).unwrap(), expected);
    for content in [
        "assert(false);\r\n",
        "assert(false);\n\n",
        "assert(true);\r\n\n",
    ] {
        let mut changed = files.clone();
        changed["files"][0]["contents"] = content.into();
        assert_ne!(saved_test_content_hash(&changed).unwrap(), expected);
    }
    let mut renamed = files;
    renamed["files"][0]["path"] = "other.test.ts".into();
    assert_ne!(saved_test_content_hash(&renamed).unwrap(), expected);
    assert_eq!(
        saved_test_content_hash(&fixture()["artifactStructuredData"]).unwrap(),
        fixture()["artifactLink"]["testContentHash"]
    );
}

#[test]
fn saved_test_hash_rejects_ambiguous_or_malformed_files() {
    for path in [
        "",
        "   ",
        "/a.ts",
        "../a.ts",
        "a/../b.ts",
        "./a.ts",
        "a//b.ts",
        "C:/a.ts",
        "a\\b.ts",
        "a\0.ts",
    ] {
        assert!(
            saved_test_content_hash(
                &serde_json::json!({"files": [{"path": path, "contents": ""}]})
            )
            .is_err(),
            "accepted {path:?}"
        );
    }
    for value in [
        serde_json::json!({}),
        serde_json::json!({"files": []}),
        serde_json::json!({"files": [{"path": "a.ts", "contents": 1}]}),
        serde_json::json!({"files": [{"path": "a.ts", "content": "wrong field"}]}),
        serde_json::json!({"files": [{"contents": ""}]}),
        serde_json::json!({"files": [{"path": "a.ts", "contents": "1"}, {"path": "a.ts", "contents": "2"}]}),
    ] {
        assert!(saved_test_content_hash(&value).is_err());
    }
}

#[tokio::test]
async fn artifact_binding_rejects_markdown_hash_and_changed_saved_test_bytes() {
    let pool = pool().await;
    let review = completed(&pool).await;
    let artifact: ReviewArtifactLink =
        serde_json::from_value(fixture()["artifactLink"].clone()).unwrap();
    let markdown = "```ts\nassert.equal(price, 2);\n```\n";
    let files = fixture()["artifactStructuredData"].clone();
    sqlx::query("INSERT INTO artifacts(id,project_id,artifact_type,title,content_md,structured_data,version,created_at,updated_at) VALUES (?,?,'test_cases','test',?,?,1,?,?)")
        .bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(markdown).bind(files.to_string()).bind(&review.created_at).bind(&review.created_at).execute(&pool).await.unwrap();
    let mut wrong = artifact.clone();
    wrong.test_content_hash = digest(markdown.as_bytes());
    assert!(link_artifact(&pool, USER, review.review_id, &wrong)
        .await
        .is_err());
    link_artifact(&pool, USER, review.review_id, &artifact)
        .await
        .unwrap();
    let mut changed = files;
    changed["files"][0]["contents"] = "assert.equal(price, 3);\n\n".into();
    sqlx::query("UPDATE artifacts SET structured_data=? WHERE id=?")
        .bind(changed.to_string())
        .bind(artifact.artifact_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    wrong.link_id = Uuid::new_v4();
    wrong.test_content_hash = artifact.test_content_hash.clone();
    assert!(link_artifact(&pool, USER, review.review_id, &wrong)
        .await
        .is_err());
    assert_eq!(
        fetch_links(&pool, USER, review.review_id).await.unwrap().0,
        vec![artifact]
    );
}

#[test]
fn snapshot_budget_counts_distinct_content_and_rejects_conflicting_identities() {
    mode_snapshot(5).validate().unwrap(); // 600 KiB distinct, 1200 KiB references.
    assert!(mode_snapshot(9).validate().is_err()); // 1080 KiB distinct.
    let base = mode_snapshot(5);
    let mut size = base.clone();
    size.sources[1].byte_length += 1;
    assert!(size.validate().is_err());
    let mut lines = base.clone();
    lines.sources[1].line_count += 1;
    assert!(lines.validate().is_err());
    let mut blob = base.clone();
    blob.sources[1].blob_id = "f".repeat(40);
    assert!(blob.validate().is_err());
    let mut content = base;
    content.sources[1].content_hash = "f".repeat(64);
    assert!(content.validate().is_err());
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
async fn second_live_pool_and_desktop_cannot_interrupt_reviews() {
    let dir = std::env::temp_dir().join(format!("tessera-live-{}", Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("review data.db");
    let owner = crate::db::AppDatabase::open(&path).await.unwrap();
    let pool = owner.pool();
    seed_pool(pool).await;
    let queued = queued(pool).await;
    let mut running = super::tests::queued(pool).await;
    running.status = ReviewStatus::Running;
    running = update(pool, USER, 0, running).await.unwrap();
    let second = crate::db::init_pool_at(&path).await.unwrap();
    // URI opening is generic too; use / separators for portable SQLite URLs.
    let url = format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"));
    let third = crate::db::init_pool(&url).await.unwrap();
    let alias = dir.join(".").join("review data.db");
    assert!(crate::db::AppDatabase::open(&alias).await.is_err());
    for review in [&queued, &running] {
        assert_eq!(
            fetch(&second, USER, review.review_id).await.unwrap(),
            *review
        );
    }
    // The first worker still accepts progress after the rejected second startup.
    running.progress = 20;
    update(pool, USER, 1, running).await.unwrap();
    third.close().await;
    second.close().await;
    pool.close().await;
    drop(owner);
    remove_test_directory(&dir).await;
}

// A subprocess owns the database with unfinished work. The parent kills it
// without Rust destructors, exercising actual OS lock and SQLite crash release.
#[tokio::test]
async fn restart_worker() {
    use std::io::Write;
    let Some(path) = std::env::var_os("TESSERA_REVIEW_RESTART_TEST_DB") else {
        return;
    };
    let owner = crate::db::AppDatabase::open(std::path::Path::new(&path))
        .await
        .unwrap();
    let pool = owner.pool();
    seed_pool(pool).await;
    completed(pool).await;
    queued(pool).await;
    let mut running = queued(pool).await;
    running.status = ReviewStatus::Running;
    update(pool, USER, 0, running).await.unwrap();
    println!("review-restart-ready");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    drop(owner);
}

#[tokio::test]
async fn real_restart_recovers_abandoned_reviews_once() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("tessera-restart-{}", Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("review.db");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "repositories::review_repo::tests::restart_worker",
            "--nocapture",
            "--test-threads=2",
        ])
        .env("TESSERA_REVIEW_RESTART_TEST_DB", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        assert!(
            output.read_line(&mut line).unwrap() > 0,
            "worker exited before ready"
        );
        if line.trim_end().ends_with("review-restart-ready") {
            break;
        }
        line.clear();
    }
    assert!(crate::db::AppDatabase::open(&path).await.is_err());
    child.kill().unwrap();
    child.wait().unwrap();
    drop(output);
    drop(child);

    // Generic pool open leaves abandoned state untouched, until app ownership.
    let generic = crate::db::init_pool_at(&path).await.unwrap();
    let template = session_fixture();
    let before = list(&generic, USER, template.project_id, 10).await.unwrap();
    assert_eq!(before.iter().filter(|r| !r.status.is_terminal()).count(), 2);
    generic.close().await;
    drop(generic);
    let owner = crate::db::AppDatabase::open(&path).await.unwrap();
    let after = list(owner.pool(), USER, template.project_id, 10)
        .await
        .unwrap();
    for old in &before {
        let next = after.iter().find(|r| r.review_id == old.review_id).unwrap();
        if old.status.is_terminal() {
            assert_eq!(old, next);
        } else {
            assert_eq!(next.status, ReviewStatus::Interrupted);
            assert_eq!(next.sequence, old.sequence + 1);
            assert_eq!(next.error.as_ref().unwrap().code, "INTERRUPTED");
        }
    }
    owner.pool().close().await;
    drop(owner);
    let reopened = crate::db::AppDatabase::open(&path).await.unwrap();
    assert_eq!(
        list(reopened.pool(), USER, template.project_id, 10)
            .await
            .unwrap(),
        after
    );
    reopened.pool().close().await;
    drop(reopened);
    remove_test_directory(&dir).await;
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
async fn legacy_run_cannot_reproduce_a_different_snapshot() {
    let pool = pool().await;
    let review = completed(&pool).await;
    let artifact: ReviewArtifactLink =
        serde_json::from_value(fixture()["artifactLink"].clone()).unwrap();
    let run: ReviewRunLink = serde_json::from_value(fixture()["runLink"].clone()).unwrap();
    let stamp = &review.created_at;
    sqlx::query("INSERT INTO artifacts(id,project_id,artifact_type,title,content_md,structured_data,version,created_at,updated_at) VALUES (?,?,'test_cases','test',?,?,1,?,?)")
        .bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(fixture()["artifactContent"].as_str().unwrap()).bind(fixture()["artifactStructuredData"].to_string()).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    link_artifact(&pool, USER, review.review_id, &artifact)
        .await
        .unwrap();
    let env = &run.environment;
    sqlx::query("INSERT INTO test_runs(id,artifact_id,project_id,status,runner,started_at,finished_at,created_at,updated_at) VALUES (?,?,?,'failed',?,?,?,?,?)")
        .bind(run.run_id.to_string()).bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(&env.runner).bind(&env.started_at).bind(&env.finished_at).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO test_run_cases(id,run_id,name,status,created_at,updated_at) VALUES (?, ?,?,'failed',?,?)")
        .bind(Uuid::new_v4().to_string()).bind(run.run_id.to_string()).bind(&run.assertion).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
    assert!(link_run(&pool, USER, review.review_id, &run).await.is_err());

    let bytes = b"export const price = 3;\n".to_vec();
    let mut other = session_fixture();
    let snapshot = other.snapshot.as_mut().unwrap();
    snapshot.snapshot_id = Uuid::new_v4();
    snapshot.sources[0].content_hash = digest(&bytes);
    snapshot.sources[0].blob_id = "d".repeat(40);
    other.findings[0].finding_id = Uuid::new_v4();
    other.findings[0].sources[0].snapshot_id = snapshot.snapshot_id;
    other.steps[0].step_id = Uuid::new_v4();
    let other = completed_from(&pool, other, bytes).await;
    let mut other_artifact = artifact.clone();
    other_artifact.link_id = Uuid::new_v4();
    other_artifact.finding_id = other.findings[0].finding_id;
    other_artifact.snapshot_id = other.snapshot.as_ref().unwrap().snapshot_id;
    link_artifact(&pool, USER, other.review_id, &other_artifact)
        .await
        .unwrap();
    let mut wrong_snapshot = run;
    wrong_snapshot.attempt_id = Uuid::new_v4();
    wrong_snapshot.artifact_link_id = other_artifact.link_id;
    assert!(link_run(&pool, USER, other.review_id, &wrong_snapshot)
        .await
        .is_err());
    assert_eq!(
        fetch_links(&pool, USER, review.review_id).await.unwrap().1,
        Vec::<ReviewRunLink>::new()
    );
    assert_eq!(
        fetch_links(&pool, USER, other.review_id).await.unwrap().1,
        Vec::<ReviewRunLink>::new()
    );
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
    sqlx::query("INSERT INTO artifacts(id,project_id,artifact_type,title,content_md,structured_data,version,created_at,updated_at) VALUES (?,?,'test_cases','test',?,?,1,?,?)")
        .bind(artifact.artifact_id.to_string()).bind(review.project_id.to_string()).bind(fixture()["artifactContent"].as_str().unwrap()).bind(fixture()["artifactStructuredData"].to_string()).bind(stamp).bind(stamp).execute(&pool).await.unwrap();
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
    // Even a matching failed assertion is unbound to executed source/test bytes.
    assert!(link_run(&pool, USER, review.review_id, &run).await.is_err());
    let mut failed_attempt = run.clone();
    failed_attempt.evidence_state = EvidenceState::Inconclusive;
    link_run(&pool, USER, review.review_id, &failed_attempt)
        .await
        .unwrap();
    assert!(link_run(&pool, USER, review.review_id, &failed_attempt)
        .await
        .is_err());
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
    assert_eq!(attempts, vec![failed_attempt, inconclusive]);
    assert_eq!(fetch(&pool, USER, review.review_id).await.unwrap(), review);
    // Project deletion can clean its own descendants despite provenance retention FKs.
    sqlx::query("DELETE FROM projects WHERE id=?")
        .bind(review.project_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
}
