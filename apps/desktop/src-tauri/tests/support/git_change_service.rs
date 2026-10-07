use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use testing_ide_lib::services::git_change_service::{capture, StagedChangeSet};
use uuid::Uuid;

pub struct Repo(pub PathBuf);

impl Repo {
    pub fn new() -> Self {
        let repo = Self(std::env::temp_dir().join(format!("tessera-capture-{}", Uuid::new_v4())));
        fs::create_dir_all(&repo.0).unwrap();
        repo.git(&["init", "--quiet"]);
        for (key, value) in [
            ("core.autocrlf", "false"),
            ("core.fileMode", "false"),
            ("commit.gpgsign", "false"),
        ] {
            repo.git(&["config", key, value]);
        }
        repo
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command.current_dir(&self.0);
        // Fixture commands must not inherit host overrides or trigger our adversarial helpers.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command.env("GIT_CONFIG_NOSYSTEM", "1").env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        );
        // A fixture commit must not leave detached maintenance changing the
        // repository while capture's byte-for-byte read-only assertions run.
        command.args([
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "user.name=Capture Test",
            "-c",
            "user.email=capture@example.invalid",
        ]);
        command.args(args);
        command
    }

    pub fn git(&self, args: &[&str]) -> Vec<u8> {
        let output = self.command(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    pub fn text(&self, args: &[&str]) -> String {
        String::from_utf8(self.git(args)).unwrap().trim().to_owned()
    }

    pub fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub fn stage(&self) {
        self.git(&["add", "-A", "--", "."]);
    }
    pub fn commit(&self) {
        self.stage();
        self.git(&["commit", "--quiet", "--allow-empty", "-m", "baseline"]);
    }

    pub async fn capture(&self) -> StagedChangeSet {
        capture(&self.0).await.unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn excluded(snapshot: &StagedChangeSet, path: &str) {
    assert!(
        snapshot
            .excluded_files
            .iter()
            .any(|file| file.path == path && !file.reason.is_empty()),
        "missing exclusion for {path}"
    );
    assert!(!snapshot.files.iter().any(|file| file
        .old
        .as_ref()
        .is_some_and(|blob| blob.path == path)
        || file.new.as_ref().is_some_and(|blob| blob.path == path)));
}

pub fn state(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}

#[tokio::test]
async fn unsupported_binary_sensitive_and_intent_to_add_are_excluded() {
    let repo = Repo::new();
    for (path, bytes) in [
        ("other.py", b"print(1)".as_slice()),
        ("binary.js", b"x\0y"),
        ("invalid.ts", b"\xff"),
        (".env.js", b"private"),
        ("config/secrets/main.js", b"private"),
        ("credentials/main.ts", b"private"),
        ("key/main.ts", b"private"),
    ] {
        repo.write(path, bytes);
    }
    repo.stage();
    repo.write("intent.js", "not staged\n");
    repo.git(&["add", "-N", "--", "intent.js"]);
    let snapshot = repo.capture().await;
    assert!(snapshot.files.is_empty());
    for path in [
        "other.py",
        "binary.js",
        "invalid.ts",
        ".env.js",
        "config/secrets/main.js",
        "credentials/main.ts",
        "key/main.ts",
        "intent.js",
    ] {
        excluded(&snapshot, path);
    }
}

#[tokio::test]
async fn gitlink_is_excluded_without_fetching() {
    let repo = Repo::new();
    repo.commit();
    let head = repo.text(&["rev-parse", "HEAD"]);
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{head},module.js"),
    ]);
    let before = state(&repo.0);
    let snapshot = repo.capture().await;
    excluded(&snapshot, "module.js");
    assert_eq!(before, state(&repo.0));
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_paths_and_symlinks_are_excluded() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let repo = Repo::new();
    let path = std::ffi::OsStr::from_bytes(b"bad-\xff.js");
    repo.write(path, "export {};\n");
    symlink("missing-outside-target", repo.0.join("link.js")).unwrap();
    repo.stage();
    let snapshot = repo.capture().await;
    assert!(snapshot.files.is_empty());
    assert_eq!(snapshot.excluded_files.len(), 2);
    assert!(snapshot
        .excluded_files
        .iter()
        .all(|file| !file.reason.is_empty()));
    excluded(&snapshot, "link.js");
    let repeated = repo.capture().await;
    let exclusions = |snapshot: &StagedChangeSet| {
        snapshot
            .excluded_files
            .iter()
            .map(|file| (file.path.clone(), file.reason.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(exclusions(&snapshot), exclusions(&repeated));
}

#[tokio::test]
async fn unreadable_required_blob_and_unmerged_index_fail() {
    let repo = Repo::new();
    repo.write("conflict.js", "one\n");
    repo.commit();
    let oid = repo.text(&["rev-parse", "HEAD:conflict.js"]);
    repo.git(&["update-index", "--force-remove", "conflict.js"]);
    let mut child = repo
        .command(&["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "100644 {oid} 1\tconflict.js\n100644 {oid} 2\tconflict.js"
    )
    .unwrap();
    assert!(child.wait().unwrap().success());
    let error = capture(&repo.0).await.err().unwrap();
    assert!(matches!(error.code(), "INVALID_INPUT" | "IO_ERROR"));
    repo.git(&["reset", "--quiet", "HEAD"]);
    let missing = "1".repeat(oid.len());
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{missing},missing.js"),
    ]);
    let error = capture(&repo.0).await.err().unwrap();
    assert!(matches!(error.code(), "INVALID_INPUT" | "IO_ERROR"));
    repo.git(&["update-index", "--force-remove", "missing.js"]);
    repo.write("conflict.js", "two\n");
    repo.git(&["add", "--", "conflict.js"]);
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{missing},.ignore"),
    ]);
    let error = capture(&repo.0).await.err().unwrap();
    assert!(matches!(error.code(), "INVALID_INPUT" | "IO_ERROR"));
}

#[tokio::test]
async fn required_invalid_or_oversized_ignore_policy_fails_safely() {
    for bytes in [vec![0xff], vec![b'#'; 128 * 1024 + 1]] {
        let repo = Repo::new();
        repo.write(".ignore", bytes);
        repo.write("main.js", "export {};\n");
        repo.stage();
        assert!(capture(&repo.0).await.is_err());
    }
}

#[tokio::test]
async fn sha256_repository_uses_its_object_format_when_supported() {
    let repo = Repo::new();
    fs::remove_dir_all(repo.0.join(".git")).unwrap();
    let output = repo
        .command(&["init", "--quiet", "--object-format=sha256"])
        .output()
        .unwrap();
    if !output.status.success() {
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("sha256"),
            "unexpected git init failure"
        );
        eprintln!(
            "Git lacks SHA-256 repository support: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    repo.git(&["config", "core.autocrlf", "false"]);
    repo.write("main.js", "export {};\n");
    repo.stage();
    let snapshot = repo.capture().await;
    assert_eq!(snapshot.git_object_format, "sha256");
    let blob = snapshot.files[0].new.as_ref().unwrap();
    assert_eq!(blob.blob_id.len(), 64);
    let expected = serde_json::json!([1, "sha256", [["bWFpbi5qcw==", "100644", 0, blob.blob_id]]]);
    assert_eq!(
        snapshot.index_fingerprint,
        hash(expected.to_string().as_bytes())
    );
}

#[tokio::test]
async fn malicious_diff_textconv_fsmonitor_and_hooks_are_never_executed() {
    let repo = Repo::new();
    repo.write(".gitattributes", "*.js diff=trap\n");
    repo.write("main.js", "old\n");
    repo.commit();
    repo.write("main.js", "new\n");
    repo.stage();
    let script = "#!/bin/sh\nprintf executed > .git/helper-ran\nexit 1\n";
    repo.write(".git/evil.sh", script);
    for hook in [
        "pre-commit",
        "post-index-change",
        "post-checkout",
        "post-merge",
    ] {
        repo.write(format!(".git/trap-hooks/{hook}"), script);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            "evil.sh",
            "trap-hooks/pre-commit",
            "trap-hooks/post-index-change",
            "trap-hooks/post-checkout",
            "trap-hooks/post-merge",
        ] {
            fs::set_permissions(
                repo.0.join(".git").join(path),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
    }
    for (key, value) in [
        ("diff.external", "sh .git/evil.sh"),
        ("diff.trap.command", "sh .git/evil.sh"),
        ("diff.trap.textconv", "sh .git/evil.sh"),
        ("core.fsmonitor", ".git/evil.sh"),
        ("core.hooksPath", ".git/trap-hooks"),
    ] {
        repo.git(&["config", key, value]);
    }
    let before = state(&repo.0);
    let snapshot = repo.capture().await;
    assert_eq!(snapshot.files.len(), 1);
    assert!(!repo.0.join(".git/helper-ran").exists());
    assert_eq!(before, state(&repo.0));
}
