//! Resolve Git from absolute host PATH entries, never the selected repository.

use super::command::invalid;
use crate::error::AppResult;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub(super) fn resolve(root: &Path) -> AppResult<PathBuf> {
    let path =
        std::env::var_os("PATH").ok_or_else(|| invalid("Git is unavailable; install Git"))?;
    from_path(root, &path)
}

fn from_path(root: &Path, path: &OsStr) -> AppResult<PathBuf> {
    for directory in std::env::split_paths(path) {
        // Relative/empty entries are interpreted against the child working
        // directory on some platforms. Never resolve them inside a repository.
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(if cfg!(windows) { "git.exe" } else { "git" });
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        if candidate.starts_with(root) || !candidate.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let Ok(metadata) = candidate.metadata() else {
                continue;
            };
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return Ok(candidate);
    }
    Err(invalid(
        "Git is unavailable outside the selected repository; install Git",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn relative_and_repository_owned_executables_are_never_selected() {
        let root = std::env::temp_dir().join(format!("tessera-git-path-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("bin")).unwrap();
        let root = root.canonicalize().unwrap();
        let name = if cfg!(windows) { "git.exe" } else { "git" };
        fs::write(root.join(name), "not trusted").unwrap();
        fs::write(root.join("bin").join(name), "not trusted").unwrap();
        let path = std::env::join_paths([
            Path::new("."),
            Path::new(""),
            root.as_path(),
            root.join("bin").as_path(),
        ])
        .unwrap();
        assert_eq!(from_path(&root, &path).unwrap_err().code(), "INVALID_INPUT");
        let real = resolve(&root).unwrap();
        assert!(real.is_absolute());
        assert!(!real.starts_with(&root));
        fs::remove_dir_all(root).unwrap();
    }
}
