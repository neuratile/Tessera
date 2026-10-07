//! Required object reads and distinct raw-source budgeting.

use std::collections::HashMap;

use super::{
    command::{invalid, limit_error, Runner},
    manifest::hash,
};
use crate::error::AppResult;

pub(super) const BLOB_LIMIT: usize = 128 * 1024;
const TOTAL_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct Blobs {
    sizes: HashMap<String, usize>,
    content: HashMap<String, Vec<u8>>,
    total: usize,
}

impl Blobs {
    pub async fn size(&mut self, git: &impl Runner, id: &str) -> AppResult<usize> {
        if let Some(size) = self.sizes.get(id) {
            return Ok(*size);
        }
        let kind = git.checked(&["cat-file", "-t", id], 32).await?;
        if kind != b"blob\n" {
            return Err(invalid("Required Git object is not a blob"));
        }
        let raw = git.checked(&["cat-file", "-s", id], 32).await?;
        let size = std::str::from_utf8(&raw)
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .ok_or_else(|| invalid("Invalid Git blob size"))?;
        self.sizes.insert(id.into(), size);
        Ok(size)
    }

    pub async fn read(&mut self, git: &impl Runner, id: &str) -> AppResult<Vec<u8>> {
        let size = self.size(git, id).await?;
        if size > BLOB_LIMIT {
            return Err(limit_error("Required ignore policy is too large"));
        }
        // Git object IDs are content-addressed within this single pinned format.
        if let Some(bytes) = self.content.get(id) {
            return Ok(bytes.clone());
        }
        let bytes = git.checked(&["cat-file", "blob", id], BLOB_LIMIT).await?;
        if bytes.len() != size {
            return Err(invalid("Required Git blob is unreadable"));
        }
        Ok(bytes)
    }

    pub fn retain(&mut self, id: &str, bytes: &[u8]) -> AppResult<()> {
        if self.content.contains_key(id) {
            return Ok(());
        }
        if self.total + bytes.len() > TOTAL_LIMIT {
            return Err(limit_error(
                "Captured source exceeds 1 MiB; stage a smaller change",
            ));
        }
        self.total += bytes.len();
        self.content.insert(id.into(), bytes.into());
        Ok(())
    }
}

pub(super) fn content_reason(bytes: &[u8]) -> Option<&'static str> {
    if bytes.contains(&0) {
        Some("binary")
    } else if std::str::from_utf8(bytes).is_err() {
        Some("non_utf8_content")
    } else {
        None
    }
}

pub(super) fn content_hash(bytes: &[u8]) -> String {
    hash(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinct_byte_budget_has_exact_boundary_and_duplicates_are_free() {
        let mut blobs = Blobs::default();
        let bytes = vec![b'a'; BLOB_LIMIT];
        for i in 0..8 {
            blobs.retain(&i.to_string(), &bytes).unwrap();
        }
        blobs.retain("0", &bytes).unwrap();
        blobs.retain("empty", b"").unwrap();
        assert_eq!(
            blobs.retain("extra", b"x").unwrap_err().code(),
            "LIMIT_EXCEEDED"
        );
        assert_eq!(content_reason(b"a\0b"), Some("binary"));
        assert_eq!(content_reason(b"\xff"), Some("non_utf8_content"));
    }
}
