//! Unified ranges from pinned blob IDs; never diff a later live index.

use super::{
    command::{invalid, Runner},
    CapturedBlob, DiffHunk,
};
use crate::error::AppResult;

pub(super) async fn capture(
    git: &impl Runner,
    old: Option<&CapturedBlob>,
    new: Option<&CapturedBlob>,
) -> AppResult<Vec<DiffHunk>> {
    match (old, new) {
        (Some(old), Some(new)) if old.blob_id == new.blob_id => Ok(Vec::new()),
        (Some(old), Some(new)) => {
            let patch = git
                .checked(
                    &[
                        "diff",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-renames",
                        "--no-color",
                        "--text",
                        "--unified=3",
                        "--diff-algorithm=myers",
                        "--no-indent-heuristic",
                        &old.blob_id,
                        &new.blob_id,
                        "--",
                    ],
                    1024 * 1024,
                )
                .await?;
            parse(&patch)
        }
        (Some(blob), None) => whole_file(blob, false),
        (None, Some(blob)) => whole_file(blob, true),
        (None, None) => Ok(Vec::new()),
    }
}

fn whole_file(blob: &CapturedBlob, addition: bool) -> AppResult<Vec<DiffHunk>> {
    let text = std::str::from_utf8(&blob.content).map_err(|_| invalid("Invalid captured text"))?;
    let count =
        u32::try_from(text.lines().count()).map_err(|_| invalid("Invalid captured line count"))?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let (old_start, old_lines, new_start, new_lines, prefix) = if addition {
        (0, 0, 1, count, '+')
    } else {
        (1, count, 0, 0, '-')
    };
    let mut patch = format!("@@ -{old_start},{old_lines} +{new_start},{new_lines} @@\n");
    for line in text.split_inclusive('\n') {
        patch.push(prefix);
        patch.push_str(line);
        if !line.ends_with('\n') {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    }
    Ok(vec![DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        patch,
    }])
}

fn parse(bytes: &[u8]) -> AppResult<Vec<DiffHunk>> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("Invalid Git patch"))?;
    let mut hunks: Vec<DiffHunk> = Vec::new();
    for line in text.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 4 || fields[3] != "@@" {
                return Err(invalid("Invalid Git hunk"));
            }
            let (old_start, old_lines) = range(fields[1], '-')?;
            let (new_start, new_lines) = range(fields[2], '+')?;
            hunks.push(DiffHunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
                patch: line.into(),
            });
        } else if let Some(hunk) = hunks.last_mut() {
            hunk.patch.push_str(line);
        }
    }
    Ok(hunks)
}

fn range(field: &str, prefix: char) -> AppResult<(u32, u32)> {
    let field = field
        .strip_prefix(prefix)
        .ok_or_else(|| invalid("Invalid Git hunk range"))?;
    let (start, lines) = field.split_once(',').unwrap_or((field, "1"));
    Ok((
        start
            .parse()
            .map_err(|_| invalid("Invalid Git hunk range"))?,
        lines
            .parse()
            .map_err(|_| invalid("Invalid Git hunk range"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_length_ranges_crlf_and_no_final_newline() {
        let blob = CapturedBlob {
            path: "a.js".into(),
            blob_id: "x".into(),
            mode: "100644".into(),
            content_hash: "x".into(),
            content: b"a\r\nb".to_vec(),
        };
        let added = whole_file(&blob, true).unwrap();
        assert_eq!(
            (
                added[0].old_start,
                added[0].old_lines,
                added[0].new_start,
                added[0].new_lines
            ),
            (0, 0, 1, 2)
        );
        assert!(added[0].patch.contains("+a\r\n+b\n\\ No newline"));
        let deleted = whole_file(&blob, false).unwrap();
        assert_eq!(
            (
                deleted[0].old_start,
                deleted[0].old_lines,
                deleted[0].new_start,
                deleted[0].new_lines
            ),
            (1, 2, 0, 0)
        );
        let parsed = parse(b"header\n@@ -1 +1,0 @@\n-a\n@@ -4,0 +4,2 @@\n+b\n+c\n").unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!((parsed[0].old_lines, parsed[0].new_lines), (1, 0));
        assert!(range("-no", '-').is_err());
    }
}
