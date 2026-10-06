//! Deterministic process-boundary races plus a minimal real-repository smoke test.

use super::*;
use async_trait::async_trait;
use command::Output;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy)]
enum Scenario {
    Stable,
    HeadRace,
    ExcludedModeRace,
    MissingBlob,
    MissingPolicy,
    IntermediateIndex,
}

struct Fixture {
    scenario: Scenario,
    index_reads: AtomicUsize,
    head_reads: AtomicUsize,
    diff_reads: AtomicUsize,
}

impl Fixture {
    fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            index_reads: AtomicUsize::new(0),
            head_reads: AtomicUsize::new(0),
            diff_reads: AtomicUsize::new(0),
        }
    }
}

fn id(digit: char) -> String {
    digit.to_string().repeat(40)
}

fn index_record(path: &str, mode: &str, oid: &str) -> String {
    format!("{mode} {oid} 0\t{path}\0  ctime: 0:0\n  mtime: 0:0\n  dev: 0\tino: 0\n  uid: 0\tgid: 0\n  size: 0\tflags: 0\n")
}

#[async_trait]
impl Runner for Fixture {
    async fn run(&self, args: &[&str], _: usize) -> AppResult<Output> {
        let bytes = match args[0] {
            "rev-parse" if args.contains(&"--is-inside-work-tree") => b"true\n".to_vec(),
            "rev-parse" if args.contains(&"--show-prefix") => b"\n".to_vec(),
            "rev-parse" if args.contains(&"--show-object-format") => b"sha1\n".to_vec(),
            "rev-parse" => {
                let call = self.head_reads.fetch_add(1, Ordering::SeqCst);
                format!(
                    "{}\n",
                    id(if call > 0 && matches!(self.scenario, Scenario::HeadRace) {
                        'c'
                    } else {
                        'b'
                    })
                )
                .into_bytes()
            }
            "ls-files" => {
                let call = self.index_reads.fetch_add(1, Ordering::SeqCst);
                let mode = if call > 0 && matches!(self.scenario, Scenario::ExcludedModeRace) {
                    "100755"
                } else {
                    "100644"
                };
                let mut data = index_record("main.js", "100644", &id('2'));
                data.push_str(&index_record("omitted.txt", mode, &id('3')));
                if matches!(self.scenario, Scenario::MissingPolicy) {
                    data.push_str(&index_record(".ignore", "100644", &id('4')));
                }
                data.into_bytes()
            }
            "ls-tree" => format!("100644 blob {}\tmain.js\0", id('1')).into_bytes(),
            "cat-file" => {
                let oid = args[2];
                if (oid == id('2') && matches!(self.scenario, Scenario::MissingBlob))
                    || (oid == id('4') && matches!(self.scenario, Scenario::MissingPolicy))
                {
                    return Ok(Output {
                        success: false,
                        bytes: Vec::new(),
                    });
                }
                match args[1] {
                    "-t" => b"blob\n".to_vec(),
                    "-s" => b"2\n".to_vec(),
                    "blob" if oid == id('1') => b"x\n".to_vec(),
                    "blob" => b"y\n".to_vec(),
                    _ => panic!("unexpected blob operation"),
                }
            }
            "diff" => {
                self.diff_reads.fetch_add(1, Ordering::SeqCst);
                assert!(args.contains(&id('1').as_str()));
                assert!(args.contains(&id('2').as_str()));
                // Simulate an intermediate live index unrelated to the pinned
                // manifests. No ls-files call is allowed while deriving hunks.
                if matches!(self.scenario, Scenario::IntermediateIndex) {
                    assert_eq!(self.index_reads.load(Ordering::SeqCst), 1);
                }
                b"diff --git a/old b/new\n@@ -1 +1 @@\n-x\n+y\n".to_vec()
            }
            _ => panic!("unexpected Git command: {}", args[0]),
        };
        Ok(Output {
            success: true,
            bytes,
        })
    }
}

#[tokio::test]
async fn deterministic_head_and_excluded_mode_races_require_new_review() {
    for scenario in [Scenario::HeadRace, Scenario::ExcludedModeRace] {
        let error = capture_with(&Fixture::new(scenario)).await.unwrap_err();
        assert_eq!(error.code(), "INVALID_INPUT");
        assert!(error.to_string().contains("capture_changed"));
        assert!(error.to_string().contains("new review"));
    }
}

#[tokio::test]
async fn pinned_diff_ignores_intermediate_live_index_and_capture_ids_are_unique() {
    let fixture = Fixture::new(Scenario::IntermediateIndex);
    let snapshot = capture_with(&fixture).await.unwrap();
    assert_eq!(fixture.diff_reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.index_reads.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.files[0].old.as_ref().unwrap().content, b"x\n");
    assert_eq!(snapshot.files[0].new.as_ref().unwrap().content, b"y\n");
    assert_eq!(snapshot.excluded_files[0].path, "omitted.txt");
    let other = capture_with(&Fixture::new(Scenario::Stable)).await.unwrap();
    assert_ne!(snapshot.snapshot_id, other.snapshot_id);
    assert_eq!(snapshot.index_fingerprint, other.index_fingerprint);
}

#[tokio::test]
async fn missing_source_and_required_policy_never_fabricate_snapshots() {
    for scenario in [Scenario::MissingBlob, Scenario::MissingPolicy] {
        assert_eq!(
            capture_with(&Fixture::new(scenario))
                .await
                .unwrap_err()
                .code(),
            "INVALID_INPUT"
        );
    }
}

#[tokio::test]
async fn real_policies_are_pinned_and_intent_to_add_is_excluded() {
    let repo = Repo::new();
    fs::create_dir(repo.0.join("src")).unwrap();
    fs::write(repo.0.join(".gitignore"), "src/old.js\n").unwrap();
    fs::write(repo.0.join("src/.ignore"), "nested.js\n").unwrap();
    for path in ["src/old.js", "src/nested.js", "allowed.js"] {
        fs::write(repo.0.join(path), "export const v = 1;\n").unwrap();
    }
    repo.git(&["add", "-f", "."]);
    repo.git(&["commit", "--quiet", "-m", "base"]);
    fs::write(repo.0.join(".gitignore"), "").unwrap();
    fs::write(repo.0.join("src/.ignore"), "").unwrap();
    for path in ["src/old.js", "src/nested.js", "allowed.js"] {
        fs::write(repo.0.join(path), "export const v = 2;\n").unwrap();
    }
    repo.git(&["add", "."]);
    fs::write(repo.0.join("intent.js"), "unstaged\n").unwrap();
    repo.git(&["add", "-N", "intent.js"]);
    fs::write(repo.0.join(".gitignore"), "*.js\n").unwrap();
    fs::write(repo.0.join(".git/info/exclude"), "allowed.js\n").unwrap();
    let snapshot = capture(&repo.0).await.unwrap();
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].new.as_ref().unwrap().path, "allowed.js");
    for path in ["src/old.js", "src/nested.js", "intent.js"] {
        assert!(snapshot.excluded_files.iter().any(|f| f.path == path));
    }
    assert!(snapshot
        .excluded_files
        .iter()
        .any(|f| f.path == "intent.js" && f.reason == "intent_to_add"));
}

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let repo =
            Self(std::env::temp_dir().join(format!("tessera-106-unit-{}", uuid::Uuid::new_v4())));
        fs::create_dir_all(&repo.0).unwrap();
        repo.git(&["init", "--quiet"]);
        repo.git(&["config", "user.name", "capture-test"]);
        repo.git(&["config", "user.email", "capture-test@example.invalid"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo
    }
    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fixture Git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn real_repository_staged_bytes_unborn_clean_and_read_only_helpers() {
    let repo = Repo::new();
    fs::write(repo.0.join("main.js"), "export const staged = 1;\r\n").unwrap();
    repo.git(&["add", "main.js"]);
    fs::write(repo.0.join("main.js"), "unstaged fix\n").unwrap();
    let index = fs::read(repo.0.join(".git/index")).unwrap();
    let first = capture(&repo.0).await.unwrap();
    assert!(first.base_commit.is_none());
    assert_eq!(
        first.files[0].new.as_ref().unwrap().content,
        b"export const staged = 1;\r\n"
    );
    assert_eq!(first.files[0].hunks[0].old_lines, 0);
    assert_eq!(fs::read(repo.0.join(".git/index")).unwrap(), index);
    repo.git(&["commit", "--quiet", "-m", "base"]);
    assert!(capture(&repo.0).await.unwrap().files.is_empty());
    fs::write(repo.0.join("main.js"), "export const staged = 2;\n").unwrap();
    repo.git(&["add", "main.js"]);
    fs::write(repo.0.join(".gitattributes"), "*.js diff=trap\n").unwrap();
    fs::write(
        repo.0.join(".git/evil.sh"),
        "#!/bin/sh\necho invoked > .git/helper-ran\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            repo.0.join(".git/evil.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    for (key, value) in [
        ("diff.external", "sh .git/evil.sh"),
        ("diff.trap.command", "sh .git/evil.sh"),
        ("diff.trap.textconv", "sh .git/evil.sh"),
        ("core.fsmonitor", "sh .git/evil.sh"),
        ("core.hooksPath", ".git/trap-hooks"),
    ] {
        repo.git(&["config", key, value]);
    }
    let index = fs::read(repo.0.join(".git/index")).unwrap();
    let head = fs::read(repo.0.join(".git/HEAD")).unwrap();
    let result = capture(&repo.0).await.unwrap();
    assert_eq!(result.files.len(), 1);
    assert!(result.files[0].hunks[0]
        .patch
        .contains("-export const staged = 1;"));
    assert!(!repo.0.join(".git/helper-ran").exists());
    assert_eq!(fs::read(repo.0.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(repo.0.join(".git/HEAD")).unwrap(), head);
}
