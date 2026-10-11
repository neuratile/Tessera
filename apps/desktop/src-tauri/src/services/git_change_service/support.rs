//! Bounded index-only context reads, pinned to an existing change capture.
use super::{
    blobs::{content_hash, content_reason, Blobs, BLOB_LIMIT},
    command::{invalid, Runner},
    manifest,
    policy::{display_path, entry_reason, Policies},
    CapturedBlob, ExcludedFile, StagedChangeSet,
};
use crate::{
    error::AppResult,
    services::{ast_service, file_discovery_service::SourceLanguage},
};
use std::collections::{BTreeMap, BTreeSet};

/// Exact optional index source and visible omissions bound to a staged capture.
#[derive(Debug, Default)]
pub struct SupportingCapture {
    pub snapshot_id: String,
    pub index_fingerprint: String,
    pub files: Vec<CapturedBlob>,
    pub omitted: Vec<ExcludedFile>,
}

pub(super) async fn capture(
    git: &impl Runner,
    snapshot: &StagedChangeSet,
    semantic_paths: &[String],
) -> AppResult<SupportingCapture> {
    let format = manifest::text(
        &git.checked(&["rev-parse", "--show-object-format"], 32)
            .await?,
    )?;
    if format != snapshot.git_object_format {
        return Err(invalid(
            "capture_changed: Git format changed; start a new review",
        ));
    }
    let head = manifest::head(git, &format).await?;
    let index = manifest::index(git, &format).await?;
    let fingerprint = manifest::fingerprint(&format, &index)?;
    if head != snapshot.base_commit || fingerprint != snapshot.index_fingerprint {
        return Err(invalid(
            "capture_changed: HEAD or index changed; start a new review",
        ));
    }
    let paths: Vec<_> = index
        .iter()
        .filter_map(|e| std::str::from_utf8(&e.path).ok())
        .collect();
    let selected = support_paths(snapshot, &paths, semantic_paths)?;
    let mut result = SupportingCapture {
        snapshot_id: snapshot.snapshot_id.clone(),
        index_fingerprint: snapshot.index_fingerprint.clone(),
        omitted: missing_paths(semantic_paths, &paths),
        ..SupportingCapture::default()
    };
    let mut blobs = Blobs::default();
    retain_required(git, snapshot, &index, &mut blobs).await?;
    // Policies only for the bounded selection; never read working-tree ignore files.
    let policy_paths: Vec<_> = selected.iter().take(20).map(String::as_bytes).collect();
    let policies = Policies::capture(git, &index, &policy_paths, &mut blobs).await?;
    for (position, path) in selected.iter().enumerate() {
        if position >= 20 {
            result.omitted.push(ExcludedFile {
                path: path.clone(),
                reason: "context_limit".into(),
            });
            continue;
        }
        let entry = index.iter().find(|e| e.path == path.as_bytes());
        let reason = if let Some(entry) = entry {
            entry_reason(entry).or_else(|| policies.ignored(&entry.path).then_some("ignored"))
        } else {
            Some("missing_index_path")
        };
        if let Some(reason) = reason {
            result.omitted.push(ExcludedFile {
                path: path.clone(),
                reason: reason.into(),
            });
            continue;
        }
        let Some(entry) = entry else {
            continue;
        };
        if blobs.size(git, &entry.id).await? > BLOB_LIMIT {
            result.omitted.push(ExcludedFile {
                path: path.clone(),
                reason: "file_too_large".into(),
            });
            continue;
        }
        let content = blobs.read(git, &entry.id).await?;
        if let Some(reason) = content_reason(&content) {
            result.omitted.push(ExcludedFile {
                path: path.clone(),
                reason: reason.into(),
            });
            continue;
        }
        blobs.retain(&entry.id, &content)?;
        result.files.push(CapturedBlob {
            path: display_path(&entry.path),
            blob_id: entry.id.clone(),
            mode: entry.mode.clone(),
            content_hash: content_hash(&content),
            content,
        });
    }
    let after_head = manifest::head(git, &format).await?;
    let after_index = manifest::index(git, &format).await?;
    super::ensure_current(
        head.as_deref(),
        &fingerprint,
        &index,
        after_head.as_deref(),
        &manifest::fingerprint(&format, &after_index)?,
        &after_index,
    )?;
    Ok(result)
}

// Stale paths remain visible but cannot consume a supporting-file slot.
fn missing_paths(semantic: &[String], index_paths: &[&str]) -> Vec<ExcludedFile> {
    let missing: BTreeSet<_> = semantic
        .iter()
        .filter(|path| {
            normalize(path).as_deref() == Some(path.as_str())
                && !index_paths.contains(&path.as_str())
        })
        .collect();
    missing
        .into_iter()
        .map(|path| ExcludedFile {
            path: path.clone(),
            reason: "missing_index_path".into(),
        })
        .collect()
}

// Original required source and policies share the distinct-byte cap with support.
async fn retain_required(
    git: &impl Runner,
    snapshot: &StagedChangeSet,
    index: &[manifest::Entry],
    blobs: &mut Blobs,
) -> AppResult<()> {
    for file in &snapshot.files {
        for blob in file.old.iter().chain(file.new.iter()) {
            blobs.retain(&blob.blob_id, &blob.content)?;
        }
    }
    let base = manifest::base(
        git,
        snapshot.base_commit.as_deref(),
        &snapshot.git_object_format,
    )
    .await?;
    let changes = super::changes::compare(&base, index)?;
    let old_paths: Vec<_> = changes
        .iter()
        .filter_map(|c| c.old.as_ref().map(|e| e.path.as_slice()))
        .collect();
    let new_paths: Vec<_> = changes
        .iter()
        .filter_map(|c| c.new.as_ref().map(|e| e.path.as_slice()))
        .collect();
    Policies::capture(git, &base, &old_paths, blobs).await?;
    Policies::capture(git, index, &new_paths, blobs).await?;
    Ok(())
}

/// Rank direct relative imports, then filename-related tests, then semantic
/// paths. All ranks have lexical ties. No working-tree scan, alias resolution,
/// transitive expansion, network, or embedding call is performed.
pub(in crate::services) fn support_paths(
    snapshot: &StagedChangeSet,
    index_paths: &[&str],
    semantic: &[String],
) -> AppResult<Vec<String>> {
    let changed: BTreeSet<_> = snapshot
        .files
        .iter()
        .flat_map(|f| f.new.iter())
        .map(|b| b.path.as_str())
        .collect();
    let present: BTreeSet<_> = index_paths.iter().copied().collect();
    let mut ranked = BTreeMap::<String, u8>::new();
    let mut stems = BTreeSet::new();
    for file in &snapshot.files {
        for blob in file.old.iter().chain(file.new.iter()) {
            stems.insert(stem(&blob.path).to_owned());
            let source = std::str::from_utf8(&blob.content)
                .map_err(|_| invalid("Invalid captured source"))?;
            for import in parse(blob, source)?.imports {
                if !import.source.starts_with('.') {
                    continue;
                }
                let parent = blob.path.rsplit_once('/').map_or("", |(p, _)| p);
                let Some(relative) = normalize(&format!("{parent}/{}", import.source)) else {
                    continue;
                };
                let mut candidates = vec![relative.clone()];
                for extension in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
                    candidates.push(format!("{relative}.{extension}"));
                    candidates.push(format!("{relative}/index.{extension}"));
                }
                if let Some(path) = candidates.iter().find(|p| present.contains(p.as_str())) {
                    stems.insert(stem(path).to_owned());
                    ranked.insert(path.clone(), 0);
                }
            }
        }
    }
    for path in index_paths {
        if is_test(path) && stems.contains(stem(path)) {
            ranked.entry((*path).into()).or_insert(1);
        }
    }
    for path in semantic {
        if normalize(path).as_deref() == Some(path.as_str()) && present.contains(path.as_str()) {
            ranked.entry(path.clone()).or_insert(2);
        }
    }
    let mut selected: Vec<_> = ranked
        .into_iter()
        .filter(|(path, _)| !changed.contains(path.as_str()))
        .collect();
    selected.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(selected.into_iter().map(|(p, _)| p).collect())
}
fn normalize(path: &str) -> Option<String> {
    let mut parts = vec![];
    for part in path.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.rsplit_once('.').map_or(name, |(n, _)| n);
    name.strip_suffix(".test")
        .or_else(|| name.strip_suffix(".spec"))
        .unwrap_or(name)
}
/// Deterministic filename/directory hint, not a claim of test discovery.
pub(in crate::services) fn is_test(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.contains(".test.")
        || name.contains(".spec.")
        || path
            .split('/')
            .any(|p| matches!(p, "test" | "tests" | "__tests__"))
}
/// Parse only supported snapshot languages without reading disk.
pub(in crate::services) fn parse(
    blob: &CapturedBlob,
    source: &str,
) -> AppResult<ast_service::ParsedFile> {
    let language = match blob.path.rsplit_once('.').map(|(_, e)| e) {
        Some("js" | "jsx" | "mjs" | "cjs") => SourceLanguage::JavaScript,
        Some("ts" | "tsx") => SourceLanguage::TypeScript,
        _ => return Ok(ast_service::ParsedFile::default()),
    };
    ast_service::parse(source, language)
}
