//! Real Git repositories exercise the staged-capture contract, not working-tree discovery.
#[path = "support/git_change_service.rs"]
mod support;

use std::{fs, path::Path};
use support::{excluded, hash, state, Repo};
use testing_ide_lib::services::git_change_service::{capture, ChangeKind};
use uuid::Uuid;

#[tokio::test]
async fn staged_checkout_regression_survives_unstaged_fix_and_capture_is_read_only() {
    let repo = Repo::new();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/fixtures/checkout");
    let baseline = fs::read(fixture.join("baseline/src/checkout.mjs")).unwrap();
    let regression = fs::read(fixture.join("regression/src/checkout.mjs")).unwrap();
    let clean = fs::read(fixture.join("clean/src/checkout.mjs")).unwrap();
    repo.write("src/checkout.mjs", &baseline);
    repo.commit();
    let head = repo.text(&["rev-parse", "HEAD"]);
    let old_oid = repo.text(&["rev-parse", "HEAD:src/checkout.mjs"]);
    repo.write("src/checkout.mjs", &regression);
    repo.stage();
    let new_oid = repo.text(&["rev-parse", ":src/checkout.mjs"]);
    repo.write("src/checkout.mjs", &clean);
    repo.write("untracked.js", "not reviewed\n");
    let before = state(&repo.0);
    let first = repo.capture().await;
    let second = repo.capture().await;
    assert_eq!(
        before,
        state(&repo.0),
        "index, objects, refs, HEAD and worktree must remain byte-identical"
    );
    assert_eq!(first.base_commit.as_deref(), Some(head.as_str()));
    assert_eq!(first.files.len(), 1);
    assert!(first.excluded_files.is_empty());
    let file = &first.files[0];
    assert!(matches!(file.change_kind, ChangeKind::Modified));
    for (blob, bytes, oid) in [
        (file.old.as_ref().unwrap(), &baseline, &old_oid),
        (file.new.as_ref().unwrap(), &regression, &new_oid),
    ] {
        assert_eq!(blob.path, "src/checkout.mjs");
        assert_eq!(blob.content, *bytes);
        assert_eq!(blob.blob_id, *oid);
        assert_eq!(blob.mode, "100644");
        assert_eq!(blob.content_hash, hash(bytes));
    }
    assert!(file.hunks.iter().any(|hunk| hunk
        .patch
        .contains("-  if (!Number.isInteger(quantity) || quantity < 1)")
        && hunk
            .patch
            .contains("+  if (!Number.isInteger(quantity) || quantity === 0)")));
    assert!(file.hunks.iter().all(|hunk| hunk.old_start > 0
        && hunk.new_start > 0
        && hunk.old_lines > 0
        && hunk.new_lines > 0));
    assert!(Uuid::parse_str(&first.snapshot_id).is_ok());
    assert!(Uuid::parse_str(&second.snapshot_id).is_ok());
    assert_ne!(first.snapshot_id, second.snapshot_id);
    assert_eq!(first.index_fingerprint, second.index_fingerprint);
}

#[tokio::test]
async fn clean_and_unborn_repositories_do_not_write_an_empty_tree() {
    let repo = Repo::new();
    let before = state(&repo.0);
    let empty = repo.capture().await;
    assert!(empty.base_commit.is_none());
    assert!(empty.files.is_empty());
    assert_eq!(before, state(&repo.0));
    repo.write("empty.js", []);
    repo.stage();
    let before = state(&repo.0);
    let added = repo.capture().await;
    assert_eq!(before, state(&repo.0));
    assert!(added.base_commit.is_none());
    assert!(matches!(added.files[0].change_kind, ChangeKind::Added));
    assert!(added.files[0].old.is_none());
    assert!(added.files[0].new.as_ref().unwrap().content.is_empty());
    repo.commit();
    let clean = repo.capture().await;
    assert!(clean.base_commit.is_some());
    assert!(clean.files.is_empty());
    assert!(clean.excluded_files.is_empty());
}

#[tokio::test]
async fn exact_rename_is_paired_but_edited_move_is_delete_and_add() {
    let repo = Repo::new();
    repo.write("exact.js", "export const exact = 1;\n");
    repo.write("edit.js", "export const edit = 2;\n");
    repo.write("delete.js", "export const deleted = 3;\n");
    repo.commit();
    fs::rename(repo.0.join("exact.js"), repo.0.join("renamed.js")).unwrap();
    fs::remove_file(repo.0.join("edit.js")).unwrap();
    fs::remove_file(repo.0.join("delete.js")).unwrap();
    repo.write("moved.js", "export const edit = 4;\n");
    repo.stage();
    let snapshot = repo.capture().await;
    assert_eq!(snapshot.files.len(), 4);
    let rename = snapshot
        .files
        .iter()
        .find(|file| matches!(file.change_kind, ChangeKind::Renamed))
        .unwrap();
    assert_eq!(rename.old.as_ref().unwrap().path, "exact.js");
    assert_eq!(rename.new.as_ref().unwrap().path, "renamed.js");
    assert_eq!(
        rename.old.as_ref().unwrap().blob_id,
        rename.new.as_ref().unwrap().blob_id
    );
    for path in ["edit.js", "delete.js"] {
        let file = snapshot
            .files
            .iter()
            .find(|file| file.old.as_ref().is_some_and(|blob| blob.path == path))
            .unwrap();
        assert!(matches!(file.change_kind, ChangeKind::Deleted));
        assert!(file.new.is_none());
        let hunk = &file.hunks[0];
        assert_eq!(
            (
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines
            ),
            (1, 1, 0, 0)
        );
    }
    let addition = snapshot
        .files
        .iter()
        .find(|file| {
            file.new
                .as_ref()
                .is_some_and(|blob| blob.path == "moved.js")
        })
        .unwrap();
    assert!(matches!(addition.change_kind, ChangeKind::Added));
    assert!(addition.old.is_none());
    let hunk = &addition.hunks[0];
    assert_eq!(
        (
            hunk.old_start,
            hunk.old_lines,
            hunk.new_start,
            hunk.new_lines
        ),
        (0, 0, 1, 1)
    );
}

#[tokio::test]
async fn paths_raw_hashes_modes_and_canonical_full_index_fingerprint() {
    let repo = Repo::new();
    for path in ["a.js", "z.txt"] {
        repo.write(path, "export {};\r\n// no final newline");
    }
    repo.stage();
    repo.git(&["update-index", "--chmod=+x", "a.js"]);
    let oid = repo.text(&["rev-parse", ":a.js"]);
    let format = repo.text(&["rev-parse", "--show-object-format"]);
    let expected = serde_json::json!([
        1,
        format,
        [
            ["YS5qcw==", "100755", 0, oid],
            ["ei50eHQ=", "100644", 0, oid]
        ]
    ]);
    let snapshot = repo.capture().await;
    assert_eq!(
        snapshot.index_fingerprint,
        hash(expected.to_string().as_bytes())
    );
    assert_eq!(snapshot.files[0].new.as_ref().unwrap().mode, "100755");
    assert_eq!(
        snapshot.files[0].new.as_ref().unwrap().content_hash,
        hash(b"export {};\r\n// no final newline")
    );
    excluded(&snapshot, "z.txt");
    repo.git(&["update-index", "--chmod=+x", "z.txt"]);
    assert_ne!(
        snapshot.index_fingerprint,
        repo.capture().await.index_fingerprint
    );
    let repo = Repo::new();
    let paths = vec![
        ("space name.js", "c3BhY2UgbmFtZS5qcw=="),
        ("日本語.ts", "5pel5pys6KqeLnRz"),
        ("-leading.js", "LWxlYWRpbmcuanM="),
        ("single'quote.ts", "c2luZ2xlJ3F1b3RlLnRz"),
    ];
    #[cfg(unix)]
    let paths = [
        paths.as_slice(),
        &[
            ("tab\tname.js", "dGFiCW5hbWUuanM="),
            ("new\nline.ts", "bmV3CmxpbmUudHM="),
            ("double\"quote.js", "ZG91YmxlInF1b3RlLmpz"),
        ],
    ]
    .concat();
    for (path, _) in &paths {
        repo.write(path, "export {};\n");
    }
    repo.stage();
    let snapshot = repo.capture().await;
    assert_eq!(snapshot.files.len(), paths.len());
    let oid = repo.text(&["rev-parse", ":-leading.js"]);
    let mut sorted = paths.clone();
    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let entries: Vec<_> = sorted
        .iter()
        .map(|(_, encoded)| serde_json::json!([encoded, "100644", 0, oid]))
        .collect();
    let expected = serde_json::json!([1, snapshot.git_object_format, entries]);
    assert_eq!(
        snapshot.index_fingerprint,
        hash(expected.to_string().as_bytes())
    );
    for (path, _) in paths {
        assert!(snapshot
            .files
            .iter()
            .any(|file| file.new.as_ref().unwrap().path == path));
    }
}

#[tokio::test]
async fn mode_only_change_preserves_both_sides() {
    let repo = Repo::new();
    repo.write("mode.js", "export {};\n");
    repo.commit();
    repo.git(&["update-index", "--chmod=+x", "mode.js"]);
    let snapshot = repo.capture().await;
    assert_eq!(snapshot.files.len(), 1);
    let file = &snapshot.files[0];
    assert!(matches!(file.change_kind, ChangeKind::Modified));
    assert_eq!(file.old.as_ref().unwrap().mode, "100644");
    assert_eq!(file.new.as_ref().unwrap().mode, "100755");
    assert_eq!(
        file.old.as_ref().unwrap().content,
        file.new.as_ref().unwrap().content
    );
    assert!(file.hunks.is_empty());
}

#[tokio::test]
async fn fifty_changes_include_exclusions_and_fifty_one_fails() {
    let repo = Repo::new();
    for i in 0..49 {
        repo.write(format!("f{i}.js"), "export {};\n");
    }
    repo.write("excluded.txt", "outside review slice\n");
    repo.stage();
    let snapshot = repo.capture().await;
    assert_eq!(snapshot.files.len(), 49);
    assert_eq!(snapshot.excluded_files.len(), 1);
    repo.write("also-excluded.txt", "outside review slice\n");
    repo.stage();
    assert_eq!(
        capture(&repo.0).await.err().unwrap().code(),
        "LIMIT_EXCEEDED"
    );
}

#[tokio::test]
async fn oversized_either_side_excludes_the_entire_change() {
    for oversized_old in [false, true] {
        let repo = Repo::new();
        repo.write(
            "size.js",
            vec![b'a'; 128 * 1024 + usize::from(oversized_old)],
        );
        repo.commit();
        repo.write(
            "size.js",
            vec![b'b'; 128 * 1024 + usize::from(!oversized_old)],
        );
        repo.stage();
        let snapshot = repo.capture().await;
        assert!(snapshot.files.is_empty());
        excluded(&snapshot, "size.js");
        assert_eq!(snapshot.excluded_files[0].reason, "file_too_large");
    }
}

#[tokio::test]
async fn one_mib_distinct_raw_bytes_is_allowed_and_duplicates_are_free() {
    let repo = Repo::new();
    for i in 0u8..5 {
        repo.write(format!("f{i}.js"), vec![b'a' + i % 4; 128 * 1024]);
    }
    repo.commit();
    for i in 0u8..5 {
        repo.write(format!("f{i}.js"), vec![b'e' + i % 4; 128 * 1024]);
    }
    repo.write("duplicate-old.js", vec![b'a'; 128 * 1024]);
    repo.write("duplicate-new.js", vec![b'e'; 128 * 1024]);
    repo.stage();
    assert_eq!(repo.capture().await.files.len(), 7);
    repo.write("extra.js", "x");
    repo.stage();
    assert_eq!(
        capture(&repo.0).await.err().unwrap().code(),
        "LIMIT_EXCEEDED"
    );
    fs::remove_file(repo.0.join("extra.js")).unwrap();
    repo.write(".ignore", "\n");
    repo.stage();
    assert_eq!(
        capture(&repo.0).await.err().unwrap().code(),
        "LIMIT_EXCEEDED",
        "required policy bytes count toward distinct capture bytes"
    );
}

#[tokio::test]
async fn pinned_ancestor_ignore_rules_not_worktree_or_info_exclude_govern_both_sides() {
    let repo = Repo::new();
    repo.write(".gitignore", "src/ignored.js\n");
    repo.write("src/.ignore", "nested.js\n");
    for path in ["src/ignored.js", "src/nested.js", "allowed.js"] {
        repo.write(path, "export const v = 1;\n");
    }
    repo.git(&["add", "-f", "--", "."]);
    repo.git(&["commit", "--quiet", "-m", "baseline"]);
    repo.write(".gitignore", "");
    repo.write("src/.ignore", "");
    for path in ["src/ignored.js", "src/nested.js", "allowed.js"] {
        repo.write(path, "export const v = 2;\n");
    }
    repo.stage();
    repo.write(".gitignore", "*.js\n");
    repo.write("src/.ignore", "*.js\n");
    repo.write(".git/info/exclude", "allowed.js\n");
    let snapshot = repo.capture().await;
    excluded(&snapshot, "src/ignored.js");
    excluded(&snapshot, "src/nested.js");
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].new.as_ref().unwrap().path, "allowed.js");
    repo.write(".gitignore", "allowed.js\n");
    repo.git(&["add", "--", ".gitignore"]);
    repo.write(".gitignore", "");
    excluded(&repo.capture().await, "allowed.js");
}

#[tokio::test]
async fn invalid_root_and_non_repository_are_rejected() {
    let repo = Repo::new();
    fs::remove_dir_all(repo.0.join(".git")).unwrap();
    for path in [
        repo.0.join("missing"),
        repo.0.join("plain"),
        repo.0.join("file.js"),
    ] {
        if path.ends_with("plain") {
            fs::create_dir(&path).unwrap();
        }
        if path.ends_with("file.js") {
            fs::write(&path, "").unwrap();
        }
        assert_eq!(capture(&path).await.err().unwrap().code(), "INVALID_INPUT");
    }
    assert_eq!(
        capture(&repo.0).await.err().unwrap().code(),
        "INVALID_INPUT"
    );
}
