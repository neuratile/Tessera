use super::types::{
    GitObjectFormat, ReviewChangeKind, ReviewOutcome, ReviewProvenance, ReviewSession,
    ReviewSnapshot, ReviewSourceReference, ReviewStatus, SourceSide,
};
use crate::error::{AppError, AppResult};
use chrono::DateTime;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub(super) fn invalid() -> AppError {
    AppError::InvalidInput("Invalid review record".into())
}
pub(super) fn ensure(ok: bool) -> AppResult<()> {
    if ok {
        Ok(())
    } else {
        Err(invalid())
    }
}
pub(super) fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 16_384 && !value.contains('\0')
}
pub(super) fn hash(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\\', ':'])
        && value
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && value.split('/').count() <= 64
}
pub(super) fn timestamp(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value).is_ok()
}
pub(super) fn provenance(value: &ReviewProvenance) -> AppResult<()> {
    ensure(text(&value.model) && text(&value.prompt_version))
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn lines(bytes: &[u8]) -> u32 {
    // Captured source uses LF logical lines, with no phantom final line.
    u32::try_from(bytes.split_inclusive(|&b| b == b'\n').count()).unwrap_or(u32::MAX)
}

impl ReviewSnapshot {
    /// Validate manifest identity and side/path uniqueness; source bytes are checked on insert.
    /// # Errors
    /// Returns invalid input for malformed or inconsistent source identities.
    pub fn validate(&self) -> AppResult<()> {
        let object_len = if self.git_object_format == GitObjectFormat::Sha1 {
            40
        } else {
            64
        };
        ensure(
            hash(&self.index_fingerprint, 64)
                && self
                    .base_commit
                    .as_ref()
                    .map_or(true, |s| hash(s, object_len)),
        )?;
        let mut keys = HashSet::new();
        let mut contents = HashMap::new();
        let mut blob_hashes = HashMap::new();
        let mut size = 0_u64;
        for source in &self.sources {
            ensure(
                path(&source.path)
                    && hash(&source.blob_id, object_len)
                    && hash(&source.content_hash, 64)
                    && matches!(source.mode.as_str(), "100644" | "100755")
                    && source.byte_length <= 128 * 1024
                    && source.line_count <= source.byte_length
                    && keys.insert((source.side as u8, source.path.as_str())),
            )?;
            // Old/new/path references may share bytes, but cannot disagree on identity.
            let identity = (
                source.blob_id.as_str(),
                source.byte_length,
                source.line_count,
            );
            if let Some(previous) = contents.insert(source.content_hash.as_str(), identity) {
                ensure(previous == identity)?;
            } else {
                size += u64::from(source.byte_length);
            }
            if let Some(previous) =
                blob_hashes.insert(source.blob_id.as_str(), source.content_hash.as_str())
            {
                ensure(previous == source.content_hash)?;
            }
        }
        ensure(size <= 1024 * 1024 && self.changes.len() <= 50 && self.context_paths.len() <= 20)?;
        let mut used = HashSet::new();
        for change in &self.changes {
            ensure(match change.change_kind {
                ReviewChangeKind::Added => change.old_path.is_none() && change.new_path.is_some(),
                ReviewChangeKind::Deleted => change.old_path.is_some() && change.new_path.is_none(),
                ReviewChangeKind::Modified => {
                    change.old_path.is_some() && change.old_path == change.new_path
                }
                ReviewChangeKind::Renamed => {
                    change.old_path.is_some()
                        && change.new_path.is_some()
                        && change.old_path != change.new_path
                }
            })?;
            for (side, p) in [
                (SourceSide::Old, &change.old_path),
                (SourceSide::New, &change.new_path),
            ] {
                if let Some(p) = p {
                    ensure(
                        keys.contains(&(side as u8, p.as_str()))
                            && used.insert((side as u8, p.as_str())),
                    )?;
                }
            }
        }
        for p in &self.context_paths {
            ensure(
                keys.contains(&(SourceSide::New as u8, p.as_str()))
                    && used.insert((SourceSide::New as u8, p.as_str())),
            )?;
        }
        ensure(used == keys)?;
        for exclusion in &self.exclusions {
            ensure(text(&exclusion.path) && text(&exclusion.reason))?;
        }
        Ok(())
    }
}

impl ReviewSourceReference {
    /// Validate against the exact stored side/blob, never working-tree coordinates.
    /// # Errors
    /// Returns invalid input for foreign snapshots, paths, or out-of-bounds lines.
    pub fn validate(&self, snapshot: &ReviewSnapshot) -> AppResult<()> {
        ensure(self.snapshot_id == snapshot.snapshot_id && path(&self.path))?;
        let source = snapshot
            .sources
            .iter()
            .find(|s| s.path == self.path && s.side == self.side)
            .ok_or_else(invalid)?;
        ensure(match (self.line_start, self.line_end) {
            (None, None) => true,
            (Some(start), Some(end)) => start > 0 && end >= start && end <= source.line_count,
            _ => false,
        })
    }
}

impl ReviewSession {
    /// Validate persisted DTO invariants independently of repository authorization.
    /// # Errors
    /// Returns invalid input for malformed progress, lifecycle, or source references.
    pub fn validate(&self) -> AppResult<()> {
        provenance(&self.provenance)?;
        ensure(
            self.progress <= 100
                && timestamp(&self.created_at)
                && timestamp(&self.updated_at)
                && (self.status == ReviewStatus::Completed) == self.outcome.is_some()
                && (self.status != ReviewStatus::Completed
                    || (self.progress == 100 && self.snapshot.is_some()))
                && (self.status != ReviewStatus::Failed || self.error.is_some()),
        )?;
        if let Some(error) = &self.error {
            ensure(text(&error.code) && text(&error.message))?;
        }
        for entry in &self.truncation {
            ensure(text(&entry.resource) && text(&entry.reason) && entry.omitted_count > 0)?;
        }
        let mut ids = HashSet::new();
        for step in &self.steps {
            ensure(
                ids.insert(step.step_id) && step.progress <= 100 && timestamp(&step.created_at),
            )?;
            if let Some(error) = &step.error {
                ensure(text(&error.code) && text(&error.message))?;
            }
        }
        if let Some(snapshot) = &self.snapshot {
            snapshot.validate()?;
        }
        ids.clear();
        for finding in &self.findings {
            ensure(
                ids.insert(finding.finding_id)
                    && text(&finding.summary)
                    && text(&finding.explanation)
                    && text(&finding.expected_behavior)
                    && !finding.sources.is_empty(),
            )?;
            let snapshot = self.snapshot.as_ref().ok_or_else(invalid)?;
            for source in &finding.sources {
                source.validate(snapshot)?;
            }
        }
        if self.outcome.is_some_and(|o| o != ReviewOutcome::Reviewed) {
            ensure(self.findings.is_empty())?;
        }
        Ok(())
    }
}
