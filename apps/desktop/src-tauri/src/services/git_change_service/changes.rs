//! Manifest comparison, exact-content rename pairing, and whole-change exclusions.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    blobs::{content_hash, content_reason, Blobs, BLOB_LIMIT},
    command::{limit_error, Runner},
    hunks,
    manifest::Entry,
    policy::{display_path, entry_reason, Policies},
    CapturedBlob, ChangeKind, ChangedFile, ExcludedFile,
};
use crate::error::AppResult;

pub(super) struct Change {
    pub old: Option<Entry>,
    pub new: Option<Entry>,
    kind: ChangeKind,
}

pub(super) fn compare(base: &[Entry], index: &[Entry]) -> AppResult<Vec<Change>> {
    let old: BTreeMap<_, _> = base.iter().map(|e| (e.path.as_slice(), e)).collect();
    let new: BTreeMap<_, _> = index.iter().map(|e| (e.path.as_slice(), e)).collect();
    let paths: BTreeSet<_> = old.keys().chain(new.keys()).copied().collect();
    let mut modifications = Vec::new();
    let mut deletions = Vec::new();
    let mut additions = Vec::new();
    for path in paths {
        match (old.get(path), new.get(path)) {
            (Some(a), Some(b)) if a.id == b.id && a.mode == b.mode && !b.intent => (),
            (Some(a), Some(b)) => modifications.push(Change {
                old: Some((*a).clone()),
                new: Some((*b).clone()),
                kind: ChangeKind::Modified,
            }),
            (Some(a), None) => deletions.push((*a).clone()),
            (None, Some(b)) => additions.push((*b).clone()),
            _ => (),
        }
    }
    // Count changed manifest paths before rename pairing, including exclusions.
    if modifications.len() + deletions.len() + additions.len() > 50 {
        return Err(limit_error(
            "More than 50 changed entries; stage a smaller change",
        ));
    }
    for addition in additions {
        let paired = deletions
            .iter()
            .position(|old| old.id == addition.id && !addition.intent);
        if let Some(position) = paired {
            modifications.push(Change {
                old: Some(deletions.remove(position)),
                new: Some(addition),
                kind: ChangeKind::Renamed,
            });
        } else {
            modifications.push(Change {
                old: None,
                new: Some(addition),
                kind: ChangeKind::Added,
            });
        }
    }
    modifications.extend(deletions.into_iter().map(|old| Change {
        old: Some(old),
        new: None,
        kind: ChangeKind::Deleted,
    }));
    modifications.sort_by(|a, b| key(a).cmp(key(b)));
    Ok(modifications)
}

fn key(change: &Change) -> &[u8] {
    change
        .new
        .as_ref()
        .or(change.old.as_ref())
        .map_or(&[], |e| &e.path)
}

pub(super) async fn capture(
    git: &impl Runner,
    changes: &[Change],
    old_policy: &Policies,
    new_policy: &Policies,
    blobs: &mut Blobs,
) -> AppResult<(Vec<ChangedFile>, Vec<ExcludedFile>)> {
    let mut files = Vec::new();
    let mut excluded = Vec::new();
    for change in changes {
        let sides: Vec<_> = change.old.iter().chain(change.new.iter()).collect();
        let mut oversized = false;
        // Even excluded regular/symlink entries must name readable Git blobs.
        // Gitlinks need not exist locally: never fetch a submodule object.
        for side in &sides {
            if !side.intent && side.mode != "160000" {
                oversized |= blobs.size(git, &side.id).await? > BLOB_LIMIT;
            }
        }
        let reason = sides
            .iter()
            .find_map(|e| entry_reason(e))
            .or_else(|| oversized.then_some("file_too_large"))
            .or_else(|| {
                let old_ignored = change
                    .old
                    .as_ref()
                    .is_some_and(|e| old_policy.ignored(&e.path));
                let new_ignored = change
                    .new
                    .as_ref()
                    .is_some_and(|e| new_policy.ignored(&e.path));
                (old_ignored || new_ignored).then_some("ignored")
            });
        if let Some(reason) = reason {
            exclude(change, reason, &mut excluded);
            continue;
        }
        let old = read(git, change.old.as_ref(), blobs).await?;
        let new = read(git, change.new.as_ref(), blobs).await?;
        if let Some(reason) = old
            .iter()
            .chain(new.iter())
            .find_map(|b| content_reason(&b.content))
        {
            exclude(change, reason, &mut excluded);
            continue;
        }
        for blob in old.iter().chain(new.iter()) {
            blobs.retain(&blob.blob_id, &blob.content)?;
        }
        let hunks = hunks::capture(git, old.as_ref(), new.as_ref()).await?;
        files.push(ChangedFile {
            change_kind: change.kind,
            old,
            new,
            hunks,
        });
    }
    excluded.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok((files, excluded))
}

async fn read(
    git: &impl Runner,
    entry: Option<&Entry>,
    blobs: &mut Blobs,
) -> AppResult<Option<CapturedBlob>> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let content = blobs.read(git, &entry.id).await?;
    Ok(Some(CapturedBlob {
        path: display_path(&entry.path),
        blob_id: entry.id.clone(),
        mode: entry.mode.clone(),
        content_hash: content_hash(&content),
        content,
    }))
}

fn exclude(change: &Change, reason: &str, excluded: &mut Vec<ExcludedFile>) {
    let mut paths = BTreeSet::new();
    for entry in change.old.iter().chain(change.new.iter()) {
        if paths.insert(&entry.path) {
            excluded.push(ExcludedFile {
                path: display_path(&entry.path),
                reason: reason.into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str, id: &str) -> Entry {
        Entry {
            path: path.as_bytes().into(),
            id: id.into(),
            mode: "100644".into(),
            stage: 0,
            intent: false,
        }
    }
    #[test]
    fn only_exact_renames_and_mode_changes_are_retained() {
        let old = [
            entry("a.js", "1"),
            entry("b.js", "2"),
            entry("mode.js", "3"),
        ];
        let mut new = [
            entry("c.js", "1"),
            entry("d.js", "4"),
            entry("mode.js", "3"),
        ];
        new[2].mode = "100755".into();
        let changes = compare(&old, &new).unwrap();
        assert_eq!(changes.len(), 4);
        assert_eq!(
            changes
                .iter()
                .filter(|c| c.kind == ChangeKind::Renamed)
                .count(),
            1
        );
        assert_eq!(
            changes
                .iter()
                .filter(|c| c.kind == ChangeKind::Modified)
                .count(),
            1
        );
        let entries: Vec<_> = (0..51).map(|i| entry(&format!("{i}.txt"), "x")).collect();
        assert_eq!(
            compare(&[], &entries).err().unwrap().code(),
            "LIMIT_EXCEEDED"
        );
        assert_eq!(compare(&[], &entries[..50]).unwrap().len(), 50);
    }
}
