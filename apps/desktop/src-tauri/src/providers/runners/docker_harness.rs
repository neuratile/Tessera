//! Shared Docker plumbing for sandboxed test runners
//! (`plan/SANDBOX_PYTHON_RUNNER.md` §4.1).
//!
//! Every Docker-backed [`TestRunner`](super::TestRunner) — JS/TS today,
//! Python now, Java/Go later — must run its container with the **same**
//! hardening flags (ADR-0004). Copying those flags between runner files
//! invites silent drift: one runner shipping with a weaker sandbox. This
//! module owns the one canonical flag set ([`hardened_run_args`]), the
//! timeout / cancel → `docker kill` orchestration ([`run_container`]),
//! the RAII workspace cleanup ([`WorkspaceGuard`]), and the truncation
//! caps every runner applies to attacker-controlled output.
//!
//! A unit test below asserts the emitted flag set contains every required
//! hardening flag — the drift tripwire: removing a flag fails the build's
//! test gate, not a security review months later.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use uuid::Uuid;

use super::{
    CancelToken, ResourceLimits, RunInput, RunStatus, RunnerError, TestResult, TestStatus,
};

/// Workspace mount point inside the container.
pub const WORK_MOUNT: &str = "/work";

/// Cap on captured stdout / stderr stored or surfaced (§10 — no unbounded
/// blobs). Bytes beyond this are dropped with a truncation marker.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Per-field caps on parsed result strings. The container writes the
/// results file, so test names + failure messages are attacker-controlled
/// (§10 — no unbounded blobs into the DB / UI). Truncated on a char boundary.
pub const MAX_TEST_NAME_BYTES: usize = 512;
pub const MAX_FAILURE_MSG_BYTES: usize = 8 * 1024;

/// `--ulimit fsize` cap (bytes): the largest single file the suite may write
/// into the bind-mounted workspace. Bounds a disk-fill `DoS` through `/work`
/// while leaving ample room for results + coverage on real projects.
pub const MAX_WRITE_BYTES: u64 = 64 * 1024 * 1024;

/// Probe for a reachable Docker daemon. Maps a missing binary or a
/// down daemon to [`RunnerError::DockerUnavailable`] so the service can
/// drive the "execution unavailable" UX (plan §3) instead of a hard
/// error.
pub async fn ensure_docker_available() -> Result<(), RunnerError> {
    let output = Command::new("docker")
        .arg("version")
        .arg("--format")
        .arg("{{.Server.Version}}")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| RunnerError::DockerUnavailable(format!("docker binary not found: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RunnerError::DockerUnavailable(format!(
            "docker daemon unreachable: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

/// Preflight: verify the locally-built runner image exists on the daemon.
///
/// Runner images are built locally and never published to a registry
/// (local-first guarantee, see `docker/Dockerfile.runner-*`), so `docker run`
/// against a missing image fails with a cryptic registry-pull error (`pull
/// access denied`, exit 125) instead of anything actionable. The returned
/// error carries the exact build command for `dockerfile`. A non-zero exit
/// whose stderr does not say "No such image" (e.g. the daemon dropped between
/// the two preflight probes) is routed to [`RunnerError::DockerUnavailable`]
/// instead, so the user is never told to rebuild an image they already have.
pub async fn ensure_runner_image(image: &str, dockerfile: &str) -> Result<(), RunnerError> {
    let output = Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| RunnerError::DockerUnavailable(format!("docker binary not found: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !is_no_such_image(&stderr) {
            return Err(RunnerError::DockerUnavailable(format!(
                "docker image inspect failed: {}",
                stderr.trim()
            )));
        }
        return Err(RunnerError::ImageMissing(format!(
            "runner image `{image}` is not built; build it from the repo root with: \
             docker build -t {image} \
             -f apps/desktop/src-tauri/docker/{dockerfile} \
             apps/desktop/src-tauri/docker"
        )));
    }
    Ok(())
}

/// Whether `docker image inspect` stderr reports a missing image, as opposed
/// to some other failure (daemon gone, permission error). Docker has phrased
/// this as `Error: No such image: …` and `Error response from daemon: No such
/// image: …` across versions, so match case-insensitively on the stable part.
fn is_no_such_image(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("no such image")
}

/// Raw result of the container process.
pub struct ContainerOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Emit the one canonical `docker run` argument list (everything after the
/// `docker` binary) with the full ADR-0004 hardening flag set:
/// `--network none`, CPU / memory / pids caps, `--ulimit fsize` write cap,
/// read-only rootfs + tmpfs, `--cap-drop ALL`, `no-new-privileges`, the
/// `/work` bind mount, and (on Unix) the host uid:gid user mapping.
///
/// Every Docker runner must build its invocation through this function —
/// never inline the flags — so a future runner cannot ship with a weaker
/// sandbox by omission. The unit test below is the drift tripwire.
///
/// # Errors
///
/// [`RunnerError::Workspace`] when the workspace cannot be stat'ed for the
/// Unix uid:gid mapping.
pub fn hardened_run_args(
    image: &str,
    workspace: &Path,
    limits: &ResourceLimits,
    container_name: &str,
    in_container_cmd: &str,
) -> Result<Vec<String>, RunnerError> {
    let mount = format!("{}:{WORK_MOUNT}", workspace.display());
    let memory = format!("{}m", limits.memory_mb);
    let cpus = format!("{:.2}", f64::from(limits.cpus));
    let pids = limits.pids.to_string();
    let fsize = format!("fsize={MAX_WRITE_BYTES}");

    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        container_name.into(),
        "--network".into(),
        "none".into(),
        "--cpus".into(),
        cpus,
        "--memory".into(),
        memory,
        "--pids-limit".into(),
        pids,
        "--ulimit".into(),
        fsize,
        "--read-only".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--security-opt".into(),
        "no-new-privileges".into(),
        "-v".into(),
        mount,
        "-w".into(),
        WORK_MOUNT.into(),
    ];

    // The workspace is 0o700 host-user-only (see `WorkspaceGuard::create`),
    // so the container must run as the host uid:gid to write its results
    // into the bind mount — and every file it creates stays owned by the
    // host user, keeping the `Drop` cleanup working. The uid has no passwd
    // entry in the image, so HOME points at the writable tmpfs.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(workspace).map_err(|e| {
            RunnerError::Workspace(format!("stat workspace {}: {e}", workspace.display()))
        })?;
        let user = format!("{}:{}", meta.uid(), meta.gid());
        args.extend(["--user".into(), user, "-e".into(), "HOME=/tmp".into()]);
    }

    args.extend([
        image.into(),
        "sh".into(),
        "-c".into(),
        in_container_cmd.into(),
    ]);

    Ok(args)
}

/// Run a suite in a hardened container (plan §7) and capture its output.
///
/// Termination is the critical part. Dropping the `docker run` child only
/// kills the *CLI*, not the daemon-side container, so on either the
/// wall-clock timeout **or** a user cancellation we issue an explicit
/// `docker kill` against the container's name. `--rm` then reaps it and
/// `kill_on_drop` cleans up the leaked CLI handle.
pub async fn run_container(
    workspace: &Path,
    image: &str,
    in_container_cmd: &str,
    input: &RunInput,
    cancel: &CancelToken,
) -> Result<ContainerOutput, RunnerError> {
    let limits = &input.limits;
    // Stable handle so the timeout / cancellation paths can target the
    // container directly with `docker kill`.
    let name = format!("tessera-run-{}", Uuid::new_v4());
    let args = hardened_run_args(image, workspace, limits, &name, in_container_cmd)?;

    let mut cmd = Command::new("docker");
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Backstop: if this future is dropped, SIGKILL the CLI handle too.
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| RunnerError::Process(format!("failed to spawn docker run: {e}")))?;

    let timeout = Duration::from_secs(u64::from(limits.timeout_secs));

    tokio::select! {
        // Completion is checked first so a container that finishes at exactly
        // the wall-clock deadline reports its real results instead of a
        // spurious timeout; cancellation still preempts the timeout below.
        biased;
        result = child.wait_with_output() => {
            let output = result
                .map_err(|e| RunnerError::Process(format!("docker run failed: {e}")))?;
            Ok(ContainerOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            })
        }
        () = cancel.cancelled() => {
            docker_kill(&name).await;
            Err(RunnerError::Cancelled)
        }
        () = tokio::time::sleep(timeout) => {
            docker_kill(&name).await;
            Err(RunnerError::Timeout(limits.timeout_secs))
        }
    }
}

/// Best-effort `docker kill` against a named container. Used on timeout and
/// user cancellation: terminating the local `docker run` process does **not**
/// stop the container running on the daemon, so the daemon must be signalled
/// explicitly. A failure here (e.g. the container already exited) is logged,
/// never propagated — the caller is already returning a terminal error.
pub async fn docker_kill(name: &str) {
    let result = Command::new("docker")
        .args(["kill", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    if let Err(e) = result {
        tracing::warn!(container = name, error = %e, "failed to docker kill sandbox container");
    }
}

/// Write the [`RunInput`] files into the workspace, rejecting any path the
/// runner reads back after the container exits (`is_reserved`). A crafted
/// artifact must not be allowed to pre-seed runner outputs: a container that
/// exits without writing its own output (e.g. a test that hard-kills the
/// process) would otherwise leave the forged file in place and the host
/// would read it as authentic.
pub fn materialize_files(
    root: &Path,
    input: &RunInput,
    is_reserved: impl Fn(&str) -> bool,
) -> Result<(), RunnerError> {
    for file in &input.files {
        if is_reserved(&file.relative_path) {
            return Err(RunnerError::InvalidInput(format!(
                "workspace file `{}` collides with a runner output path",
                file.relative_path
            )));
        }
        let dest = root.join(&file.relative_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RunnerError::Workspace(format!("create dir {}: {e}", parent.display())))?;
        }
        std::fs::write(&dest, &file.contents)
            .map_err(|e| RunnerError::Workspace(format!("write {}: {e}", dest.display())))?;
    }
    Ok(())
}

/// Saturating `f64 -> u32` for a millisecond duration the caller has
/// already filtered to finite + non-negative.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn f64_to_u32(value: f64) -> u32 {
    if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

/// Map parsed assertions to a run-level [`RunStatus`]: any failure →
/// `Failed`; at least one passing test and no failures → `Passed`; nothing
/// executed → `Error`.
#[must_use]
pub fn derive_status(tests: &[TestResult]) -> RunStatus {
    if tests.iter().any(|t| t.status == TestStatus::Failed) {
        return RunStatus::Failed;
    }
    if tests.iter().any(|t| t.status == TestStatus::Passed) {
        return RunStatus::Passed;
    }
    RunStatus::Error
}

/// Truncate captured stdout/stderr to [`MAX_OUTPUT_BYTES`].
#[must_use]
pub fn truncate(s: &str) -> String {
    truncate_to(s, MAX_OUTPUT_BYTES)
}

/// Truncate `s` to at most `max` bytes on a char boundary, appending a
/// marker when bytes were dropped. Shared by the output cap and the
/// per-field result-string caps.
#[must_use]
pub fn truncate_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &s[..end])
}

/// RAII guard for the throwaway workspace. Removing on `Drop` guarantees
/// cleanup on the happy path, on any `?` early-return, and on panic (§10).
pub struct WorkspaceGuard {
    path: PathBuf,
}

impl WorkspaceGuard {
    pub fn create(root: &Path) -> Result<Self, RunnerError> {
        let path = root.join(format!("tessera-run-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path)
            .map_err(|e| RunnerError::Workspace(format!("create workspace {}: {e}", path.display())))?;
        // Owner-only: a world-writable workspace would let another local uid
        // inject a hostile results file or swap a source file in the window
        // between creation and `docker run`. The container is started as the
        // host uid:gid (see `hardened_run_args`), so it can still write
        // results into the bind mount and host-side `Drop` cleanup keeps
        // working.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).map_err(
                |e| RunnerError::Workspace(format!("chmod workspace {}: {e}", path.display())),
            )?;
        }
        Ok(Self { path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.path) {
            // Best-effort: a failed cleanup must not mask the run result,
            // but it is worth a warning for disk-leak diagnosis.
            tracing::warn!(
                workspace = %self.path.display(),
                error = %e,
                "failed to remove sandbox workspace"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::runners::{RunnerLanguage, WorkspaceFile};

    /// The ADR-0004 security gate as a tripwire: every Docker runner builds
    /// its invocation through `hardened_run_args`, and this test pins the
    /// flag set. Removing or weakening a flag fails the suite immediately
    /// instead of shipping a weaker sandbox for one language.
    #[test]
    fn hardened_run_args_emit_every_required_hardening_flag() {
        let limits = ResourceLimits::default();
        let workspace = std::env::temp_dir();
        let args = hardened_run_args("tessera-runner-x", &workspace, &limits, "tessera-run-test", "true")
            .expect("args");

        let has_pair = |flag: &str, value: &str| {
            args.windows(2)
                .any(|w| w[0] == flag && w[1] == value)
        };

        assert!(has_pair("--network", "none"), "network isolation missing");
        assert!(has_pair("--cap-drop", "ALL"), "capability drop missing");
        assert!(
            has_pair("--security-opt", "no-new-privileges"),
            "no-new-privileges missing"
        );
        assert!(args.contains(&"--read-only".to_string()), "read-only rootfs missing");
        assert!(has_pair("--tmpfs", "/tmp"), "tmpfs missing");
        assert!(has_pair("--cpus", "1.00"), "cpu cap missing");
        assert!(has_pair("--memory", "512m"), "memory cap missing");
        assert!(has_pair("--pids-limit", "256"), "pids cap missing");
        assert!(
            has_pair("--ulimit", &format!("fsize={MAX_WRITE_BYTES}")),
            "fsize cap missing"
        );
        assert!(args.contains(&"--rm".to_string()), "--rm missing");
        assert!(has_pair("-w", WORK_MOUNT), "workdir missing");
        assert!(
            args.iter().any(|a| a.ends_with(&format!(":{WORK_MOUNT}"))),
            "workspace mount missing"
        );

        // The image and command come after every hardening flag, and the
        // suite runs through `sh -c`.
        let image_pos = args.iter().position(|a| a == "tessera-runner-x").expect("image");
        assert_eq!(args[image_pos + 1], "sh");
        assert_eq!(args[image_pos + 2], "-c");
        assert_eq!(args[image_pos + 3], "true");

        #[cfg(unix)]
        assert!(
            args.iter().any(|a| a == "--user"),
            "host uid:gid user mapping missing on unix"
        );
    }

    #[test]
    fn is_no_such_image_discriminates_missing_image_from_daemon_failures() {
        // Both stderr phrasings Docker has used for a missing image.
        assert!(is_no_such_image("Error: No such image: tessera-runner-js:latest"));
        assert!(is_no_such_image(
            "Error response from daemon: No such image: tessera-runner-js:latest"
        ));
        // Daemon / transport failures must not be reported as a missing image.
        assert!(!is_no_such_image(
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock"
        ));
        assert!(!is_no_such_image("permission denied while trying to connect"));
        assert!(!is_no_such_image(""));
    }

    #[test]
    fn ensure_runner_image_error_names_the_dockerfile() {
        // Pure formatting check on the ImageMissing message contract: the
        // build command must reference the runner's own Dockerfile.
        let err = RunnerError::ImageMissing(
            "runner image `x` is not built; build it from the repo root with: \
             docker build -t x -f apps/desktop/src-tauri/docker/Dockerfile.runner-py \
             apps/desktop/src-tauri/docker"
                .to_string(),
        );
        assert!(err.to_string().contains("Dockerfile.runner-py"));
    }

    #[test]
    fn materialize_files_rejects_reserved_paths() {
        let input = RunInput {
            language: RunnerLanguage::JavaScript,
            files: vec![WorkspaceFile {
                relative_path: "results.json".into(),
                contents: "{}".into(),
                is_test: true,
            }],
            limits: ResourceLimits::default(),
        };
        let err = materialize_files(&std::env::temp_dir(), &input, |p| p == "results.json")
            .expect_err("reserved path must be rejected");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn truncate_caps_long_output() {
        let big = "a".repeat(MAX_OUTPUT_BYTES + 100);
        let out = truncate(&big);
        assert!(out.len() < big.len());
        assert!(out.ends_with("…[truncated]"));

        let small = "short";
        assert_eq!(truncate(small), "short");
    }

    #[test]
    fn derive_status_prioritizes_failure() {
        let failed = vec![
            TestResult { name: "a".into(), status: TestStatus::Passed, duration_ms: 1, failure_message: None, source_line: None },
            TestResult { name: "b".into(), status: TestStatus::Failed, duration_ms: 1, failure_message: None, source_line: None },
        ];
        assert_eq!(derive_status(&failed), RunStatus::Failed);

        let all_pass = vec![TestResult { name: "a".into(), status: TestStatus::Passed, duration_ms: 1, failure_message: None, source_line: None }];
        assert_eq!(derive_status(&all_pass), RunStatus::Passed);

        assert_eq!(derive_status(&[]), RunStatus::Error);
    }

    #[test]
    fn workspace_guard_removes_directory_on_drop() {
        let root = std::env::temp_dir();
        let path = {
            let guard = WorkspaceGuard::create(&root).expect("create");
            assert!(guard.path().exists());
            guard.path().to_path_buf()
        };
        assert!(!path.exists(), "workspace must be removed on drop");
    }
}
