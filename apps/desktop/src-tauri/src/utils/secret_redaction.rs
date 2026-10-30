//! Conservative source redaction before review context assembly. Never changes lines.
//! This is not an authorization boundary: all surviving source remains untrusted.

/// Mask entire credential-bearing lines and PEM blocks, retaining CR/LF layout.
/// Reject unterminated PEM or ambiguous multiline credential values rather than
/// sending continuation lines whose boundaries cannot be established safely.
#[must_use]
pub fn redact_source(source: &str) -> Option<String> {
    let mut pem: Option<String> = None;
    let mut result = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let lower = line.to_ascii_lowercase();
        if let Some((_, rest)) = lower.split_once("-----begin ") {
            let (label, _) = rest.split_once("-----")?;
            if pem.is_some() || label.is_empty() {
                return None;
            }
            pem = Some(label.into());
        }
        let sensitive = pem.is_some()
            || [
                "password",
                "passwd",
                "secret",
                "token",
                "api_key",
                "apikey",
                "api-key",
                "authorization",
                "bearer ",
                "sk-",
                "ghp_",
                "gho_",
                "ghu_",
                "ghs_",
                "ghr_",
                "github_pat_",
                "glpat-",
                "xoxb-",
                "xoxp-",
                "npm_",
                "hf_",
                "akia",
                "aiza",
                "postgres://",
                "postgresql://",
                "mysql://",
                "mongodb://",
                "redis://",
            ]
            .iter()
            .any(|s| lower.contains(s));
        if sensitive {
            if pem.is_none() && ambiguous_value(line) {
                return None;
            }
            // Mask characters, not terminators; no source coordinates are shifted.
            result.extend(
                line.chars()
                    .map(|c| if matches!(c, '\r' | '\n') { c } else { '*' }),
            );
        } else {
            result.push_str(line);
        }
        if pem
            .as_ref()
            .is_some_and(|label| lower.contains(&format!("-----end {label}-----")))
        {
            pem = None;
        }
    }
    pem.is_none().then_some(result)
}

fn ambiguous_value(line: &str) -> bool {
    // Deliberately exclude ambiguous declarations instead of parsing arbitrary
    // languages. This also covers template literals and YAML block scalars.
    if let Some((_, value)) = line.split_once('=').or_else(|| line.split_once(':')) {
        let value = value.trim();
        if value.is_empty() || value.starts_with(['|', '>']) || value.ends_with('\\') {
            return true;
        }
    }
    ['\'', '"', '`'].into_iter().any(|quote| {
        let mut escaped = false;
        let mut count = 0;
        for c in line.chars() {
            if !escaped && c == quote {
                count += 1;
            }
            escaped = !escaped && c == '\\';
        }
        count % 2 != 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_credentials_and_multiline_keys_without_moving_lines() {
        let raw = "const api_key = 'synthetic';\r\n-----BEGIN PRIVATE KEY-----\nsynthetic\n-----END PRIVATE KEY-----\nconst ok = 1;\n";
        let safe = redact_source(raw).unwrap();
        assert!(!safe.contains("synthetic"));
        assert!(safe.ends_with("const ok = 1;\n"));
        assert_eq!(
            raw.chars()
                .filter(|c| matches!(c, '\r' | '\n'))
                .collect::<String>(),
            safe.chars()
                .filter(|c| matches!(c, '\r' | '\n'))
                .collect::<String>()
        );
        assert!(redact_source("-----BEGIN PRIVATE KEY-----\nincomplete").is_none());
        assert!(
            redact_source("-----BEGIN PRIVATE KEY-----\n-----END CERTIFICATE-----\nsynthetic")
                .is_none()
        );
    }
    #[test]
    fn rejects_ambiguous_multiline_values_and_masks_opaque_credentials() {
        for source in [
            "const apiKey =\n  'synthetic';\n",
            "secret: |\n  synthetic\n",
            "const token = `first\nsynthetic`;\n",
        ] {
            assert!(redact_source(source).is_none());
        }
        for prefix in [
            "ghp_",
            "gho_",
            "xoxb-",
            "glpat-",
            "AKIA",
            "AIza",
            "postgres://",
        ] {
            let safe = redact_source(&format!(
                "const value = '{prefix}synthetic';\r\nconst ok = 1;"
            ))
            .unwrap();
            assert!(!safe.contains("synthetic"));
            assert!(safe.ends_with("\r\nconst ok = 1;"));
        }
    }
}
