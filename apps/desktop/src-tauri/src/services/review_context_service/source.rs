//! Source mapping and prompt rendering over redacted, immutable evidence.
use super::{CapturedBlob, Excerpt, ExcludedFile, ReviewContext, Side, StagedChangeSet};
use crate::error::{AppError, AppResult};
use crate::services::git_change_service::parse;
use std::collections::BTreeSet;

pub(super) fn excerpt(
    b: &CapturedBlob,
    side: Side,
    start: u32,
    end: u32,
    text: String,
    required: bool,
) -> Excerpt {
    Excerpt {
        path: b.path.clone(),
        side,
        blob_id: b.blob_id.clone(),
        content_hash: b.content_hash.clone(),
        start_line: start,
        end_line: end,
        text,
        required,
    }
}
pub(super) fn render(excerpts: &[Excerpt]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for e in excerpts {
        digest.update(e.text.as_bytes());
        digest.update([0]);
    }
    // Deterministic, collision-free labels avoid accidental source delimiter
    // confusion, but do not authorize source instructions or prevent injection.
    let mut boundary = format!("TESSERA_{:x}", digest.finalize());
    while excerpts.iter().any(|e| e.text.contains(&boundary)) {
        boundary = format!("TESSERA_{:x}", Sha256::digest(boundary.as_bytes()));
    }
    let mut output = String::new();
    let footer = format!("\nEND SOURCE {boundary}\n");
    for e in excerpts {
        let header = format!(
            "\nSOURCE {boundary} path={} side={} blob={} hash={} lines={}-{}\n",
            serde_json::Value::String(e.path.clone()),
            if e.side == Side::Old { "old" } else { "new" },
            e.blob_id,
            e.content_hash,
            e.start_line,
            e.end_line,
        );
        output.push_str(&header);
        output.push_str(&e.text);
        output.push_str(&footer);
    }
    output
}
pub(super) fn safe_source(
    blob: &CapturedBlob,
    redact: &impl Fn(&str) -> Option<String>,
) -> AppResult<Option<String>> {
    use sha2::{Digest, Sha256};
    if format!("{:x}", Sha256::digest(&blob.content)) != blob.content_hash {
        return Err(invalid("Captured source hash mismatch"));
    }
    let source =
        std::str::from_utf8(&blob.content).map_err(|_| invalid("Invalid captured source"))?;
    let Some(safe) = redact(source) else {
        return Ok(None);
    };
    // Match terminators per logical line, not just their total count.
    let endings = |s: &str| {
        s.split_inclusive('\n')
            .map(|l| {
                if l.ends_with("\r\n") {
                    2
                } else {
                    i32::from(l.ends_with('\n'))
                }
            })
            .collect::<Vec<_>>()
    };
    if safe.len() > 128 * 1024
        || endings(source) != endings(&safe)
        || source.bytes().filter(|b| *b == b'\r').count()
            != safe.bytes().filter(|b| *b == b'\r').count()
    {
        return Ok(None);
    }
    Ok(Some(safe))
}
/// Map required hunks/symbols; return optional metadata-only source separately.
pub(super) fn append_changed(
    snapshot: &StagedChangeSet,
    excluded: &BTreeSet<&str>,
    redact: &impl Fn(&str) -> Option<String>,
    result: &mut ReviewContext,
) -> AppResult<Vec<Excerpt>> {
    let mut metadata_source = vec![];
    for file in &snapshot.files {
        if file
            .old
            .iter()
            .chain(file.new.iter())
            .any(|b| excluded.contains(b.path.as_str()))
        {
            continue;
        }
        let mut excerpts = vec![];
        let mut unmappable = false;
        for (side, blob) in [
            (Side::Old, file.old.as_ref()),
            (Side::New, file.new.as_ref()),
        ] {
            let Some(blob) = blob else { continue };
            let Some(safe) = safe_source(blob, redact)? else {
                unmappable = true;
                break;
            };
            let source = std::str::from_utf8(&blob.content)
                .map_err(|_| invalid("Invalid captured source"))?;
            let lines: Vec<_> = safe.split_inclusive('\n').collect();
            let parsed = parse(blob, source)?;
            if (parsed.language.is_none() || parsed.error_count > 0)
                && !result
                    .limitations
                    .contains(&"symbols_unavailable_or_partial")
            {
                result.limitations.push("symbols_unavailable_or_partial");
            }
            let mut ranges = vec![];
            for hunk in &file.hunks {
                let (start, length) = match side {
                    Side::Old => (hunk.old_start, hunk.old_lines),
                    Side::New => (hunk.new_start, hunk.new_lines),
                };
                if length == 0 {
                    continue;
                }
                let end = start
                    .checked_add(length - 1)
                    .ok_or_else(|| invalid("Invalid captured hunk"))?;
                ranges.push((start, end));
                for symbol in &parsed.declarations {
                    if symbol.start_line <= end && symbol.end_line >= start {
                        ranges.push((symbol.start_line, symbol.end_line));
                    }
                }
            }
            // No changed code exists for mode-only changes/exact renames.
            // Manifest metadata remains in the snapshot; new source is optional.
            if file.hunks.is_empty() && side == Side::New && !lines.is_empty() {
                excerpts.push(excerpt(
                    blob,
                    side,
                    1,
                    u32::try_from(lines.len())
                        .map_err(|_| invalid("Invalid captured line count"))?,
                    safe.clone(),
                    false,
                ));
            }
            for (start, end) in merge(ranges) {
                if start == 0 || end as usize > lines.len() {
                    return Err(invalid("Captured range is outside source"));
                }
                excerpts.push(excerpt(
                    blob,
                    side,
                    start,
                    end,
                    lines[(start - 1) as usize..end as usize].concat(),
                    true,
                ));
            }
        }
        if unmappable {
            for blob in file.old.iter().chain(file.new.iter()) {
                result.omitted.push(ExcludedFile {
                    path: blob.path.clone(),
                    reason: "redaction_unmappable".into(),
                });
            }
        } else {
            for e in excerpts {
                if e.required {
                    result.excerpts.push(e);
                } else {
                    metadata_source.push(e);
                }
            }
        }
    }
    Ok(metadata_source)
}

fn merge(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut result: Vec<(u32, u32)> = vec![];
    for (start, end) in ranges {
        if let Some(last) = result.last_mut() {
            if start <= last.1.saturating_add(1) {
                last.1 = last.1.max(end);
                continue;
            }
        }
        result.push((start, end));
    }
    result
}
pub(super) fn invalid(message: &str) -> AppError {
    AppError::InvalidInput(message.into())
}
pub(super) fn limit(message: &str) -> AppError {
    AppError::LimitExceeded(message.into())
}
