//! Immutable, local-only capture of staged JS/TS changes against pinned HEAD.
//! No working-tree source, Git writes, model calls, or serialized DTOs live here.

mod blobs;
mod changes;
mod command;
mod executable;
mod hunks;
mod manifest;
mod policy;

use std::{path::Path, time::Duration};

use crate::error::{AppError, AppResult};
use command::{invalid, limit_error, Git, Runner};

/// Owned raw-local snapshot; source bytes are not safe to send to a model
/// without a separate line-preserving redaction step.
#[derive(Debug)]
pub struct StagedChangeSet {
    /// Unique capture UUID, not a content identity.
    pub snapshot_id: String,
    /// Git object hash format (`sha1` or `sha256`).
    pub git_object_format: String,
    /// Pinned base commit; absent for an unborn branch.
    pub base_commit: Option<String>,
    /// SHA-256 of canonical compact JSON `[1, format, full_index_entries]`.
    pub index_fingerprint: String,
    /// Included staged changes with exact raw source bytes.
    pub files: Vec<ChangedFile>,
    /// Omitted paths and stable, source-free exclusion reasons.
    pub excluded_files: Vec<ExcludedFile>,
}

/// Manifest-level identity of a change; only exact-content moves are renamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// Present only in the index.
    Added,
    /// Same path with a different blob or file mode.
    Modified,
    /// Present only in the base.
    Deleted,
    /// Deleted and added paths share the exact Git blob identity.
    Renamed,
}

/// A whole changed file. Excluding either side omits the whole change.
#[derive(Debug)]
pub struct ChangedFile {
    /// Kind derived from the pinned manifests.
    pub change_kind: ChangeKind,
    /// Base-side source, absent for additions.
    pub old: Option<CapturedBlob>,
    /// Index-side source, absent for deletions.
    pub new: Option<CapturedBlob>,
    /// Unified hunks referencing the corresponding old/new logical lines.
    pub hunks: Vec<DiffHunk>,
}

/// Exact Git source bytes; never normalized or redacted during local capture.
#[derive(Debug)]
pub struct CapturedBlob {
    /// Lexically safe, case-preserving repository-relative UTF-8 path.
    pub path: String,
    /// Git object ID in the snapshot's object format.
    pub blob_id: String,
    /// Six-digit Git file mode, including executable-bit changes.
    pub mode: String,
    /// Lowercase SHA-256 over `content` before any transformations.
    pub content_hash: String,
    /// Owned source bytes, bounded to 128 KiB per blob.
    pub content: Vec<u8>,
}

/// Unified hunk ranges. Zero-length ranges retain Git's insertion/deletion
/// anchor (which may be zero); nonempty line ranges are one-based.
#[derive(Debug)]
pub struct DiffHunk {
    /// Old-side starting line or empty-range anchor.
    pub old_start: u32,
    /// Number of old-side logical lines.
    pub old_lines: u32,
    /// New-side starting line or empty-range anchor.
    pub new_start: u32,
    /// Number of new-side logical lines.
    pub new_lines: u32,
    /// Raw unified hunk header/body, including no-final-newline markers.
    pub patch: String,
}

/// An omitted changed path. Non-UTF-8 names use `base64:<padded raw bytes>`.
#[derive(Debug)]
pub struct ExcludedFile {
    /// Repository-relative path or lossless non-UTF-8 display encoding.
    pub path: String,
    /// Stable machine-readable exclusion reason, never source text.
    pub reason: String,
}

/// Capture the full stage-0 index compared with HEAD, without modifying Git.
///
/// `root` must be a repository/worktree root, not a nested directory. Captures
/// at most 50 changed manifest paths (before rename pairing), 128 KiB per blob,
/// and 1 MiB of distinct included source/required ignore-policy bytes. Policies
/// come only from applicable ancestor `.gitignore`/`.ignore` blobs on each side.
/// Reviewable paths are bounded to 4096 bytes and 64 components; distinct
/// ignore-ancestor prefix strings are bounded to 256 KiB per side.
///
/// # Errors
/// Returns `INVALID_INPUT` for invalid repositories, unmerged/missing objects,
/// or `capture_changed` (explicitly start a new review; no silent retry), and
/// `LIMIT_EXCEEDED` for resource ceilings/timeouts. Errors never expose Git
/// stderr or source. Later index/working-tree edits cannot alter returned bytes.
pub async fn capture(root: &Path) -> AppResult<StagedChangeSet> {
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| invalid("Open a valid Git repository root"))?;
    if !tokio::fs::metadata(&root)
        .await
        .map_err(|_| invalid("Open a valid Git repository root"))?
        .is_dir()
    {
        return Err(invalid("Open a valid Git repository root"));
    }
    let git = Git::new(&root)?;
    tokio::time::timeout(Duration::from_secs(60), capture_with(&git))
        .await
        .map_err(|_| limit_error("Git capture timed out; start a new review"))?
}

async fn capture_with(git: &impl Runner) -> AppResult<StagedChangeSet> {
    if git
        .checked(&["rev-parse", "--is-inside-work-tree"], 32)
        .await?
        != b"true\n"
        || !git
            .checked(&["rev-parse", "--show-prefix"], 4096)
            .await?
            .iter()
            .all(|b| *b == b'\n' || *b == b'\r')
    {
        return Err(invalid(
            "Open a Git repository root, not a bare repository or nested folder",
        ));
    }
    let format = manifest::text(
        &git.checked(&["rev-parse", "--show-object-format"], 32)
            .await?,
    )?;
    if !matches!(format.as_str(), "sha1" | "sha256") {
        return Err(invalid("Unsupported Git object format"));
    }
    let head = manifest::head(git, &format).await?;
    let index = manifest::index(git, &format).await?;
    let fingerprint = manifest::fingerprint(&format, &index)?;
    let base = manifest::base(git, head.as_deref(), &format).await?;
    let changes = changes::compare(&base, &index)?;
    let mut blobs = blobs::Blobs::default();
    let old_paths: Vec<_> = changes
        .iter()
        .filter_map(|c| c.old.as_ref().map(|e| e.path.as_slice()))
        .collect();
    let new_paths: Vec<_> = changes
        .iter()
        .filter_map(|c| c.new.as_ref().map(|e| e.path.as_slice()))
        .collect();
    let old_policy = policy::Policies::capture(git, &base, &old_paths, &mut blobs).await?;
    let new_policy = policy::Policies::capture(git, &index, &new_paths, &mut blobs).await?;
    let (files, excluded_files) =
        changes::capture(git, &changes, &old_policy, &new_policy, &mut blobs).await?;
    let after_head = manifest::head(git, &format).await?;
    let after_index = manifest::index(git, &format).await?;
    ensure_current(
        head.as_deref(),
        &fingerprint,
        &index,
        after_head.as_deref(),
        &manifest::fingerprint(&format, &after_index)?,
        &after_index,
    )?;
    Ok(StagedChangeSet {
        snapshot_id: uuid::Uuid::new_v4().to_string(),
        git_object_format: format,
        base_commit: head,
        index_fingerprint: fingerprint,
        files,
        excluded_files,
    })
}

fn ensure_current(
    before_head: Option<&str>,
    before_hash: &str,
    before_index: &[manifest::Entry],
    after_head: Option<&str>,
    after_hash: &str,
    after_index: &[manifest::Entry],
) -> AppResult<()> {
    if before_head != after_head || before_hash != after_hash || before_index != after_index {
        return Err(AppError::InvalidInput(
            "capture_changed: HEAD or index changed; start a new review".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
