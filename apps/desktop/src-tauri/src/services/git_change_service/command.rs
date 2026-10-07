//! Read-only, bounded Git plumbing. Never include command output in errors.

use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};

use async_trait::async_trait;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

use crate::error::{AppError, AppResult};

pub(super) const MANIFEST_LIMIT: usize = 8 * 1024 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;
const COMMAND_TIME: Duration = Duration::from_secs(5);

pub(super) struct Output {
    pub success: bool,
    pub bytes: Vec<u8>,
}

#[async_trait]
pub(super) trait Runner: Sync {
    async fn run(&self, args: &[&str], limit: usize) -> AppResult<Output>;

    async fn checked(&self, args: &[&str], limit: usize) -> AppResult<Vec<u8>> {
        let output = self.run(args, limit).await?;
        if !output.success {
            return Err(invalid("Git could not read the required repository data"));
        }
        Ok(output.bytes)
    }
}

pub(super) fn invalid(message: &str) -> AppError {
    AppError::InvalidInput(message.into())
}

pub(super) fn limit_error(message: &str) -> AppError {
    AppError::LimitExceeded(message.into())
}

pub(super) struct Git<'a> {
    root: &'a Path,
    executable: OsString,
}

impl<'a> Git<'a> {
    pub fn new(root: &'a Path, executable: OsString) -> Self {
        Self { root, executable }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.current_dir(self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Overrides such as GIT_INDEX_FILE must never select a different snapshot.
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
            {
                cmd.env_remove(key);
            }
        }
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        cmd.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", null)
            .env("GIT_CONFIG_GLOBAL", null)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_GRAFT_FILE", null)
            .env("GIT_NO_LAZY_FETCH", "1")
            // An empty transport allowlist also overrides per-protocol local
            // configuration, including partial-clone lazy-fetch fallbacks.
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C");
        cmd.args([
            "--no-pager",
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.hooksPath=",
        ])
        .arg("-c")
        .arg(format!("core.attributesFile={null}"))
        .args([
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.file.allow=never",
            "-c",
            "protocol.ext.allow=never",
            "-c",
            "diff.external=",
            "-c",
            "core.pager=cat",
        ])
        .args(args);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW; no repository helper processes.
        cmd
    }
}

#[async_trait]
impl Runner for Git<'_> {
    async fn run(&self, args: &[&str], limit: usize) -> AppResult<Output> {
        collect(self.command(args), limit, COMMAND_TIME).await
    }
}

async fn collect(mut command: Command, limit: usize, duration: Duration) -> AppResult<Output> {
    let mut child = command
        .spawn()
        .map_err(|_| invalid("Git is unavailable; install Git and open a Git repository"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("Git output unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| invalid("Git output unavailable"))?;
    let result = timeout(duration, async {
        let (bytes, _, status) = tokio::try_join!(
            bounded_read(stdout, limit),
            bounded_read(stderr, STDERR_LIMIT),
            async {
                child
                    .wait()
                    .await
                    .map_err(|_| invalid("Git process failed"))
            }
        )?;
        Ok(Output {
            success: status.success(),
            bytes,
        })
    })
    .await;
    match result {
        Ok(Ok(output)) => Ok(output),
        other => {
            let _ = child.start_kill();
            let _ = timeout(COMMAND_TIME, child.wait()).await;
            match other {
                Ok(Err(error)) => Err(error),
                _ => Err(limit_error("Git capture timed out; start a new review")),
            }
        }
    }
}

async fn bounded_read(
    reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| invalid("Git output could not be read"))?;
    if bytes.len() > limit {
        return Err(limit_error("Git capture output exceeds its safety limit"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_reader_rejects_overflow_and_allows_boundary() {
        assert_eq!(bounded_read(&b"abc"[..], 3).await.unwrap(), b"abc");
        assert_eq!(
            bounded_read(&b"abcd"[..], 3).await.unwrap_err().code(),
            "LIMIT_EXCEEDED"
        );
    }

    #[tokio::test]
    async fn missing_git_is_safe_invalid_input() {
        let git = Git {
            root: Path::new("."),
            executable: "tessera-nonexistent-git-106".into(),
        };
        let error = git.run(&["status"], 10).await.err().unwrap();
        assert_eq!(error.code(), "INVALID_INPUT");
        assert!(!error.to_string().contains("tessera-nonexistent"));
    }

    // Launch this test binary, not shell/repository code, to exercise real pipe
    // overflow and process cancellation without changing global environment.
    #[test]
    fn process_fixture() {
        use std::io::Write;
        match std::env::var("TESSERA_106_PROCESS_FIXTURE").as_deref() {
            Ok("stdout") => std::io::stdout().write_all(&vec![b'x'; 64 * 1024]).unwrap(),
            Ok("stderr") => std::io::stderr().write_all(&vec![b'x'; 64 * 1024]).unwrap(),
            Ok("sleep") => std::thread::sleep(Duration::from_secs(60)),
            _ => (),
        }
    }

    #[tokio::test]
    async fn process_output_and_runtime_limits_kill_and_reap_child() {
        for case in ["stdout", "stderr", "sleep"] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "services::git_change_service::command::tests::process_fixture",
                    "--nocapture",
                ])
                .env("TESSERA_106_PROCESS_FIXTURE", case)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let duration = if case == "sleep" {
                Duration::from_millis(100)
            } else {
                COMMAND_TIME
            };
            let error = collect(command, 16 * 1024, duration).await.err().unwrap();
            assert_eq!(error.code(), "LIMIT_EXCEEDED");
            if case == "sleep" {
                assert!(error.to_string().contains("timed out"));
            } else {
                assert!(error.to_string().contains("output"));
            }
        }
    }

    #[tokio::test]
    async fn helpers_and_network_are_disabled_without_shell() {
        let root = Path::new(".").canonicalize().unwrap();
        let executable = super::super::executable::resolve(&root).await.unwrap();
        let git = Git::new(&root, executable.into_os_string());
        let command = git.command(&["diff", "--no-ext-diff", "--no-textconv", "a", "b"]);
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        for flag in [
            "core.fsmonitor=false",
            "core.hooksPath=",
            "protocol.allow=never",
            "--no-ext-diff",
            "--no-textconv",
        ] {
            assert!(args.iter().any(|arg| arg == flag));
        }
        let env: Vec<_> = command.as_std().get_envs().collect();
        assert!(env
            .iter()
            .any(|(k, v)| *k == "GIT_NO_LAZY_FETCH" && v == &Some(std::ffi::OsStr::new("1"))));
        assert!(env
            .iter()
            .any(|(k, v)| *k == "GIT_ALLOW_PROTOCOL" && v == &Some(std::ffi::OsStr::new(""))));
    }
}
