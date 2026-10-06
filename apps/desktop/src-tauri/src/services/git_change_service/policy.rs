//! Lexical path safety and ignore rules read exclusively from pinned Git blobs.

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use super::{
    blobs::{content_reason, Blobs},
    command::{invalid, Runner},
    manifest::{base64, Entry},
};
use crate::error::AppResult;

pub(super) fn path_reason(raw: &[u8]) -> Option<&'static str> {
    let Ok(path) = std::str::from_utf8(raw) else {
        return Some("non_utf8_path");
    };
    if path.is_empty()
        || path.contains(['\\', '\0', ':'])
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.eq_ignore_ascii_case(".git"))
    {
        return Some("unsafe_path");
    }
    None
}

pub(super) fn display_path(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(path) => path.into(),
        Err(_) => format!("base64:{}", base64(raw)),
    }
}

pub(super) fn sensitive(path: &str) -> bool {
    path.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        part == ".env"
            || part.starts_with(".env.")
            || matches!(
                part.as_str(),
                "secret" | "secrets" | "credential" | "credentials" | "key" | "keys"
            )
    })
}

pub(super) fn entry_reason(entry: &Entry) -> Option<&'static str> {
    if let Some(reason) = path_reason(&entry.path) {
        return Some(reason);
    }
    if entry.intent {
        return Some("intent_to_add");
    }
    match entry.mode.as_str() {
        "120000" => return Some("symlink"),
        "160000" => return Some("submodule"),
        "100644" | "100755" => (),
        _ => return Some("unsupported_mode"),
    }
    let path = display_path(&entry.path);
    if sensitive(&path) {
        return Some("sensitive_path");
    }
    let extension = path.rsplit_once('.').map(|(_, ext)| ext);
    if !matches!(extension, Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx")) {
        return Some("unsupported_extension");
    }
    None
}

#[derive(Default)]
pub(super) struct Policies(BTreeMap<String, Gitignore>);

impl Policies {
    pub async fn capture(
        git: &impl Runner,
        manifest: &[Entry],
        paths: &[&[u8]],
        blobs: &mut Blobs,
    ) -> AppResult<Self> {
        let entries: BTreeMap<_, _> = manifest.iter().map(|e| (e.path.as_slice(), e)).collect();
        let mut directories = BTreeSet::new();
        for raw in paths {
            if path_reason(raw).is_some() {
                continue;
            }
            let path = display_path(raw);
            directories.insert(String::new());
            for (i, _) in path.match_indices('/') {
                directories.insert(path[..i].to_owned());
            }
        }
        let mut policies = BTreeMap::new();
        for directory in directories {
            let mut builder = GitignoreBuilder::new(Path::new(&directory));
            builder.allow_unclosed_class(false);
            for filename in [".gitignore", ".ignore"] {
                let path = if directory.is_empty() {
                    filename.into()
                } else {
                    format!("{directory}/{filename}")
                };
                let Some(entry) = entries.get(path.as_bytes()) else {
                    continue;
                };
                if entry.intent || !matches!(entry.mode.as_str(), "100644" | "100755") {
                    return Err(invalid(
                        "Required ignore policy is not a staged regular file",
                    ));
                }
                let bytes = blobs.read(git, &entry.id).await?;
                if content_reason(&bytes).is_some() {
                    return Err(invalid("Required ignore policy is invalid"));
                }
                blobs.retain(&entry.id, &bytes)?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| invalid("Required ignore policy is invalid"))?;
                for line in text.lines() {
                    builder
                        .add_line(Some(Path::new(&path).into()), line)
                        .map_err(|_| {
                            invalid("Required ignore policy contains an invalid pattern")
                        })?;
                }
            }
            policies.insert(
                directory,
                builder
                    .build()
                    .map_err(|_| invalid("Required ignore policy is invalid"))?,
            );
        }
        Ok(Self(policies))
    }

    pub fn ignored(&self, raw: &[u8]) -> bool {
        if path_reason(raw).is_some() {
            return false;
        }
        let path = display_path(raw);
        // Walk lexical ancestors, so a descendant whitelist cannot revive an
        // ignored directory (and no filesystem link resolution is involved).
        let ends = path
            .match_indices('/')
            .map(|(i, _)| (i, true))
            .chain(std::iter::once((path.len(), false)));
        for (end, is_dir) in ends {
            let node = &path[..end];
            let mut ignored = false;
            let mut directories = vec![""];
            directories.extend(node.match_indices('/').map(|(i, _)| &node[..i]));
            for directory in directories {
                if let Some(policy) = self.0.get(directory) {
                    let matched = policy.matched(node, is_dir);
                    if !matched.is_none() {
                        ignored = matched.is_ignore();
                    }
                }
            }
            if ignored {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_paths_and_sensitive_components() {
        for path in [
            "../a.js",
            "/a.js",
            "C:/a.js",
            "a\\b.js",
            "a//b.js",
            "a/./b.js",
            ".git/a.js",
            "x\0.js",
        ] {
            assert_eq!(path_reason(path.as_bytes()), Some("unsafe_path"));
        }
        assert_eq!(path_reason(b"bad-\xff.js"), Some("non_utf8_path"));
        for path in ["-a.js", "日本語.ts", "a\nb.js", "space name.js"] {
            assert!(path_reason(path.as_bytes()).is_none());
        }
        for path in [
            ".env.js",
            "src/Secrets/a.ts",
            "key/a.js",
            "credentials/a.ts",
        ] {
            assert!(sensitive(path));
        }
    }

    #[test]
    fn ancestor_precedence_and_ignored_parent_cannot_be_revived() {
        let mut root = GitignoreBuilder::new("");
        root.add_line(None, "*.js")
            .unwrap()
            .add_line(None, "blocked/")
            .unwrap();
        let mut nested = GitignoreBuilder::new("src");
        nested.add_line(None, "!allowed.js").unwrap();
        let mut blocked = GitignoreBuilder::new("blocked");
        blocked.add_line(None, "!allowed.js").unwrap();
        let policies = Policies(BTreeMap::from([
            (String::new(), root.build().unwrap()),
            ("src".into(), nested.build().unwrap()),
            ("blocked".into(), blocked.build().unwrap()),
        ]));
        assert!(policies.ignored(b"src/other.js"));
        assert!(!policies.ignored(b"src/allowed.js"));
        assert!(policies.ignored(b"blocked/allowed.js"));
    }
}
