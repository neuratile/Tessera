//! Capture-to-persistence regression: repeated mode-only sources share the byte budget.
#[path = "support/git_change_service.rs"]
mod support;

use sqlx::sqlite::SqlitePoolOptions;
use support::Repo;
use testing_ide_lib::repositories::review_repo::{
    self, GitObjectFormat, ReviewChange, ReviewChangeKind, ReviewSession, ReviewSnapshot,
    ReviewSource, ReviewSourceReference, SourceSide,
};
use uuid::Uuid;

#[tokio::test]
async fn five_distinct_mode_only_files_capture_and_persist_with_600_kib() {
    let repo = Repo::new();
    for i in 0..5 {
        let mut bytes = format!("// file {i}\n").into_bytes();
        bytes.resize(120 * 1024, b'x');
        repo.write(format!("file{i}.ts"), bytes);
    }
    repo.commit();
    repo.git(&[
        "update-index",
        "--chmod=+x",
        "--",
        "file0.ts",
        "file1.ts",
        "file2.ts",
        "file3.ts",
        "file4.ts",
    ]);
    let captured = repo.capture().await;
    assert_eq!(captured.files.len(), 5);
    assert!(captured.excluded_files.is_empty());
    let mut snapshot = ReviewSnapshot {
        snapshot_id: Uuid::parse_str(&captured.snapshot_id).unwrap(),
        git_object_format: GitObjectFormat::Sha1,
        base_commit: captured.base_commit,
        index_fingerprint: captured.index_fingerprint,
        sources: vec![],
        changes: vec![],
        context_paths: vec![],
        exclusions: vec![],
    };
    let mut source_bytes = vec![];
    for file in captured.files {
        let old = file.old.unwrap();
        let new = file.new.unwrap();
        assert_eq!(old.blob_id, new.blob_id);
        assert_eq!(old.content, new.content);
        assert_eq!(old.mode, "100644");
        assert_eq!(new.mode, "100755");
        snapshot.changes.push(ReviewChange {
            change_kind: ReviewChangeKind::Modified,
            old_path: Some(old.path.clone()),
            new_path: Some(new.path.clone()),
        });
        for (side, blob) in [(SourceSide::Old, old), (SourceSide::New, new)] {
            snapshot.sources.push(ReviewSource {
                path: blob.path,
                side,
                blob_id: blob.blob_id,
                mode: blob.mode,
                content_hash: blob.content_hash,
                byte_length: u32::try_from(blob.content.len()).unwrap(),
                line_count: 2,
            });
            source_bytes.push(blob.content);
        }
    }
    assert_eq!(
        snapshot.sources.iter().map(|s| s.byte_length).sum::<u32>(),
        1200 * 1024
    );
    snapshot.validate().unwrap();
    let template: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../packages/shared/src/schemas/fixtures/review.json"
    ))
    .unwrap();
    let template: ReviewSession = serde_json::from_value(template["session"].clone()).unwrap();
    let user = "00000000-0000-4000-8000-000000000001";
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    testing_ide_lib::db::run_migrations(&pool).await.unwrap();
    sqlx::query("INSERT INTO projects(id,user_id,name,root_path,created_at,updated_at) VALUES (?,?,'review','/tmp',?,?)")
        .bind(template.project_id.to_string()).bind(user).bind(&template.created_at).bind(&template.created_at).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO user_provider_configs(id,user_id,provider,is_active,created_at,updated_at) VALUES (?,?,'ollama',1,?,?)")
        .bind(template.provenance.provider_config_id.to_string()).bind(user).bind(&template.created_at).bind(&template.created_at).execute(&pool).await.unwrap();
    let review = review_repo::create(&pool, user, template.project_id, template.provenance)
        .await
        .unwrap();
    review_repo::attach_snapshot(&pool, user, review.review_id, &snapshot, &source_bytes)
        .await
        .unwrap();
    assert_eq!(
        review_repo::fetch(&pool, user, review.review_id)
            .await
            .unwrap()
            .snapshot,
        Some(snapshot.clone())
    );
    for (source, expected) in snapshot.sources.iter().zip(source_bytes) {
        let reference = ReviewSourceReference {
            snapshot_id: snapshot.snapshot_id,
            path: source.path.clone(),
            side: source.side,
            line_start: None,
            line_end: None,
        };
        assert_eq!(
            review_repo::fetch_source(&pool, user, review.review_id, &reference)
                .await
                .unwrap(),
            expected
        );
    }
}
