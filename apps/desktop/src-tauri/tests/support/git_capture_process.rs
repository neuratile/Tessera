//! Process isolation proves Git environment overrides cannot redirect capture.

use super::support::{state, Repo};
use std::{fs, path::Path, process::Command};
use testing_ide_lib::services::git_change_service::capture;

#[test]
fn capture_child() {
    let Some(root) = std::env::var_os("TESSERA_106_CAPTURE_ROOT") else {
        return;
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(capture(Path::new(&root)));
    if std::env::var("TESSERA_106_EXPECT_INVALID").is_ok() {
        assert_eq!(result.unwrap_err().code(), "INVALID_INPUT");
    } else {
        let snapshot = result.unwrap();
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].new.as_ref().unwrap().path, "main.js");
        assert_eq!(snapshot.files[0].new.as_ref().unwrap().content, b"staged\n");
        assert_eq!(
            snapshot.index_fingerprint,
            std::env::var("TESSERA_106_EXPECT_INDEX").unwrap()
        );
        assert_eq!(
            snapshot.base_commit,
            Some(std::env::var("TESSERA_106_EXPECT_HEAD").unwrap())
        );
    }
}

fn child(repo: &Repo) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "process_tests::capture_child", "--nocapture"])
        .env("TESSERA_106_CAPTURE_ROOT", &repo.0);
    command
}

fn run_child(mut command: Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "isolated capture failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[tokio::test]
async fn inherited_index_repository_and_config_overrides_cannot_redirect_capture() {
    let repo = Repo::new();
    repo.write("main.js", "base\n");
    repo.commit();
    repo.write("main.js", "staged\n");
    repo.stage();
    let alternate = Repo::new();
    alternate.write("other.js", "other repository\n");
    alternate.commit();
    repo.write(
        ".git/env-trap.sh",
        "#!/bin/sh\necho invoked > .git/env-helper-ran\n",
    );
    let snapshot = repo.capture().await;
    let before = state(&repo.0);
    let alternate_before = state(&alternate.0);
    let mut command = child(&repo);
    command
        .env("TESSERA_106_EXPECT_INDEX", snapshot.index_fingerprint)
        .env("TESSERA_106_EXPECT_HEAD", snapshot.base_commit.unwrap())
        .env("GIT_DIR", alternate.0.join(".git"))
        .env("GIT_COMMON_DIR", alternate.0.join(".git"))
        .env("GIT_WORK_TREE", &alternate.0)
        .env("GIT_INDEX_FILE", alternate.0.join(".git/index"))
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_0", "sh .git/env-trap.sh");
    run_child(command);
    assert!(!repo.0.join(".git/env-helper-ran").exists());
    assert_eq!(state(&repo.0), before);
    assert_eq!(state(&alternate.0), alternate_before);
}

#[test]
fn missing_promisor_blob_cannot_invoke_transport_or_modify_repository() {
    let repo = Repo::new();
    repo.write("main.js", "base\n");
    repo.commit();
    repo.write("main.js", "staged\n");
    repo.stage();
    let oid = repo.text(&["rev-parse", ":main.js"]);
    repo.write(
        ".git/fetch-trap.sh",
        "#!/bin/sh\necho invoked > .git/transport-ran\nexit 1\n",
    );
    for (key, value) in [
        ("extensions.partialClone", "origin"),
        ("remote.origin.promisor", "true"),
        ("remote.origin.partialCloneFilter", "blob:none"),
        ("remote.origin.url", "ext::sh .git/fetch-trap.sh"),
        ("protocol.ext.allow", "always"),
    ] {
        repo.git(&["config", key, value]);
    }
    let object = repo.0.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
    fs::remove_file(object).unwrap();
    let before = state(&repo.0);
    let mut command = child(&repo);
    // Host overrides actively request transport; the collector must remove them.
    command
        .env("TESSERA_106_EXPECT_INVALID", "1")
        .env("GIT_NO_LAZY_FETCH", "0")
        .env("GIT_ALLOW_PROTOCOL", "ext")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.ext.allow")
        .env("GIT_CONFIG_VALUE_0", "always");
    run_child(command);
    assert!(!repo.0.join(".git/transport-ran").exists());
    assert_eq!(state(&repo.0), before);
}
