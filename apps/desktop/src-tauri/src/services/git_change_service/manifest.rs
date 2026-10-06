//! Pinned raw-path manifests and the versioned, canonical index fingerprint.

use sha2::{Digest, Sha256};

use super::command::{invalid, Runner, MANIFEST_LIMIT};
use crate::error::AppResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub path: Vec<u8>,
    pub mode: String,
    pub stage: u8,
    pub id: String,
    pub intent: bool,
}

pub(super) async fn head(git: &impl Runner, format: &str) -> AppResult<Option<String>> {
    let output = git
        .run(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"], 256)
        .await?;
    if output.success {
        let id = text(&output.bytes)?;
        validate_id(&id, format)?;
        return Ok(Some(id));
    }
    // An absent symbolic branch is unborn; a broken/detached HEAD is not.
    let reference = text(
        &git.checked(&["symbolic-ref", "--quiet", "HEAD"], 4096)
            .await?,
    )?;
    if !reference.starts_with("refs/heads/") {
        return Err(invalid("Invalid repository HEAD"));
    }
    let refs = git
        .checked(
            &["for-each-ref", "--format=%(refname)", "--", &reference],
            MANIFEST_LIMIT,
        )
        .await?;
    if !refs.is_empty() {
        return Err(invalid("Required base commit is unreadable"));
    }
    Ok(None)
}

pub(super) async fn index(git: &impl Runner, format: &str) -> AppResult<Vec<Entry>> {
    let bytes = git
        .checked(
            &["ls-files", "--full-name", "--stage", "--debug", "-z"],
            MANIFEST_LIMIT,
        )
        .await?;
    parse_index(&bytes, format)
}

fn parse_index(mut bytes: &[u8], format: &str) -> AppResult<Vec<Entry>> {
    let mut entries = Vec::new();
    while !bytes.is_empty() {
        let nul = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| invalid("Invalid index manifest"))?;
        let record = &bytes[..nul];
        bytes = &bytes[nul + 1..];
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| invalid("Invalid index manifest"))?;
        let header =
            std::str::from_utf8(&record[..tab]).map_err(|_| invalid("Invalid index manifest"))?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3 {
            return Err(invalid("Invalid index manifest"));
        }
        let stage = fields[2]
            .parse::<u8>()
            .map_err(|_| invalid("Invalid index stage"))?;
        if stage != 0 {
            return Err(invalid("Unmerged index; resolve conflicts before review"));
        }
        // --debug includes exactly five metadata lines, after each NUL path.
        let mut flags = None;
        for line_number in 0..5 {
            let end = bytes
                .iter()
                .position(|b| *b == b'\n')
                .ok_or_else(|| invalid("Invalid index flags"))?;
            if line_number == 4 {
                let line = std::str::from_utf8(&bytes[..end])
                    .map_err(|_| invalid("Invalid index flags"))?;
                flags = line
                    .rsplit_once("flags: ")
                    .and_then(|(_, value)| u32::from_str_radix(value, 16).ok());
            }
            bytes = &bytes[end + 1..];
        }
        let flags = flags.ok_or_else(|| invalid("Invalid index flags"))?;
        entries.push(entry(
            &record[tab + 1..],
            fields[0],
            fields[1],
            stage,
            flags & 0x2000_0000 != 0,
            format,
        )?);
    }
    sort_validate(&mut entries)?;
    Ok(entries)
}

pub(super) async fn base(
    git: &impl Runner,
    head: Option<&str>,
    format: &str,
) -> AppResult<Vec<Entry>> {
    let Some(head) = head else {
        return Ok(Vec::new());
    };
    let bytes = git
        .checked(
            &["ls-tree", "-r", "-z", "--full-tree", head],
            MANIFEST_LIMIT,
        )
        .await?;
    let mut entries = Vec::new();
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| invalid("Invalid base manifest"))?;
        let header =
            std::str::from_utf8(&record[..tab]).map_err(|_| invalid("Invalid base manifest"))?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3 {
            return Err(invalid("Invalid base manifest"));
        }
        entries.push(entry(
            &record[tab + 1..],
            fields[0],
            fields[2],
            0,
            false,
            format,
        )?);
    }
    sort_validate(&mut entries)?;
    Ok(entries)
}

fn entry(
    path: &[u8],
    mode: &str,
    id: &str,
    stage: u8,
    intent: bool,
    format: &str,
) -> AppResult<Entry> {
    if mode.len() != 6 || !mode.bytes().all(|b| (b'0'..=b'7').contains(&b)) || path.is_empty() {
        return Err(invalid("Invalid Git manifest entry"));
    }
    validate_id(id, format)?;
    Ok(Entry {
        path: path.into(),
        mode: mode.into(),
        stage,
        id: id.into(),
        intent,
    })
}

fn sort_validate(entries: &mut [Entry]) -> AppResult<()> {
    entries.sort_by(|a, b| (&a.path, a.stage).cmp(&(&b.path, b.stage)));
    if entries
        .windows(2)
        .any(|w| w[0].path == w[1].path && w[0].stage == w[1].stage)
    {
        return Err(invalid("Duplicate Git manifest entry"));
    }
    Ok(())
}

pub(super) fn validate_id(id: &str, format: &str) -> AppResult<()> {
    let length = match format {
        "sha1" => 40,
        "sha256" => 64,
        _ => return Err(invalid("Unsupported Git object format")),
    };
    if id.len() != length
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("Invalid Git object identity"));
    }
    Ok(())
}

pub(super) fn text(bytes: &[u8]) -> AppResult<String> {
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| invalid("Invalid Git metadata"))?
        .trim_end_matches(['\n', '\r'])
        .to_owned())
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn fingerprint(format: &str, entries: &[Entry]) -> AppResult<String> {
    let entries: Vec<_> = entries
        .iter()
        .map(|e| serde_json::json!([base64(&e.path), e.mode, e.stage, e.id]))
        .collect();
    Ok(hash(&serde_json::to_vec(&serde_json::json!([
        1, format, entries
    ]))?))
}

// No base64 dependency is needed for this one canonical encoding boundary.
pub(super) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        result.push(char::from(ALPHABET[usize::from(a >> 2)]));
        result.push(char::from(ALPHABET[usize::from((a & 3) << 4 | b >> 4)]));
        result.push(if chunk.len() > 1 {
            char::from(ALPHABET[usize::from((b & 15) << 2 | c >> 6)])
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            char::from(ALPHABET[usize::from(c & 63)])
        } else {
            '='
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn padded_encoding_and_canonical_fingerprint() {
        for (raw, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("日本語.ts", "5pel5pys6KqeLnRz"),
        ] {
            assert_eq!(base64(raw.as_bytes()), encoded);
        }
        let id = "a".repeat(40);
        let data = format!("100755 {id} 0\tz.js\0  ctime: 0:0\n  mtime: 0:0\n  dev: 0\tino: 0\n  uid: 0\tgid: 0\n  size: 0\tflags: 20004000\n100644 {id} 0\ta.js\0  ctime: 0:0\n  mtime: 0:0\n  dev: 0\tino: 0\n  uid: 0\tgid: 0\n  size: 0\tflags: 0\n");
        let entries = parse_index(data.as_bytes(), "sha1").unwrap();
        assert!(entries[1].intent);
        assert_eq!(
            fingerprint("sha1", &entries).unwrap(),
            hash(
                serde_json::json!([
                    1,
                    "sha1",
                    [["YS5qcw==", "100644", 0, id], ["ei5qcw==", "100755", 0, id]]
                ])
                .to_string()
                .as_bytes()
            )
        );
        assert!(parse_index(data.replace(" 0\t", " 2\t").as_bytes(), "sha1").is_err());
    }
}
