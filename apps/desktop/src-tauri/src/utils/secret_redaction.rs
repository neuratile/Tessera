//! Conservative source redaction before review context assembly. Never changes lines.
//! This is not an authorization boundary: all surviving source remains untrusted.

/// Mask entire credential-bearing lines and PEM blocks, retaining CR/LF layout.
/// Reject unterminated PEM or ambiguous multiline credential values rather than
/// sending continuation lines whose boundaries cannot be established safely.
#[must_use]
pub fn redact_source(source: &str) -> Option<String> {
    let mut pem: Option<String> = None;
    let mut result = String::with_capacity(source.len());
    for raw_line in source.split_inclusive('\n') {
        let line = redact_http_userinfo(raw_line);
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
            ]
            .iter()
            .any(|s| lower.contains(s))
            || [
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
            .any(|prefix| {
                lower.match_indices(prefix).any(|(index, _)| {
                    lower[..index]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
                })
            });
        if sensitive {
            if pem.is_none() && ambiguous_value(&line) {
                return None;
            }
            // Mask characters, not terminators; no source coordinates are shifted.
            result.extend(
                line.chars()
                    .map(|c| if matches!(c, '\r' | '\n') { c } else { '*' }),
            );
        } else {
            result.push_str(&line);
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
    let trimmed = line.trim_start();
    let comment = ["//", "#", "/*", "*", "<!--"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let declaration = line
        .split_once('=')
        .filter(|(target, value)| {
            !target.trim_end().ends_with(['!', '<', '>', '='])
                && !value.starts_with(['=', '>'])
                && (comment || !ambiguous_expression(target))
        })
        .or_else(|| {
            line.split_once(':').filter(|(target, _)| {
                !target.contains('(') && !target.trim_start().starts_with("class ")
            })
        });
    let bare_target = trimmed
        .trim_end()
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let declaration_head = [
        "const ",
        "let ",
        "var ",
        "export const ",
        "export let ",
        "pub const ",
        "pub static ",
        "static ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
        || [
            "token",
            "password",
            "passwd",
            "secret",
            "api_key",
            "apikey",
            "authorization",
            "access_token",
            "refresh_token",
            "accesstoken",
            "refreshtoken",
        ]
        .contains(&bare_target.as_str());
    // A function name or parameter mentioning passwords is not itself a
    // credential value. Keep masking that line without excluding its body.
    if declaration.is_none() && !declaration_head {
        return false;
    }
    // Deliberately exclude ambiguous declarations instead of parsing arbitrary
    // languages. This also covers template literals and YAML block scalars.
    if let Some((_, value)) = declaration {
        if (value.contains("=>") && !value.contains(['\'', '"', '`']))
            || value.trim_start().starts_with("function")
            || value.trim_start().starts_with("lambda ")
        {
            return false;
        }
        // Comments can hide an unfinished expression, including block comments
        // spanning later lines. Exclude uncertain declarations rather than try
        // to parse comments/strings across arbitrary source languages.
        if ambiguous_expression(value) {
            return true;
        }
        let value = value.trim();
        if value.is_empty()
            || value.starts_with(['|', '>'])
            || value.ends_with([
                '\\', '(', '[', '{', '+', '-', '*', '/', '%', '&', '|', '^', '?', ',', ':', '=',
                '<', '>', '.',
            ])
        {
            return true;
        }
    }
    // Apostrophes/quotes in comments are prose, not string delimiters. Keep
    // masking the line, and retain the value-continuation guard above.
    if ["//", "#", "/*", "*", "<!--"]
        .iter()
        .any(|prefix| line.trim_start().starts_with(prefix))
    {
        return false;
    }
    // A balanced line need not finish its expression: JS/TS permits `''`
    // followed by `+ 'credential'`, a property access, or a tagged template.
    // Without a language parser, require an explicit same-line terminator.
    // Other languages may omit semicolons; exclude uncertain source visibly
    // instead of guessing where a credential-bearing declaration ends.
    if !line.trim_end().ends_with(';') {
        return true;
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

fn redact_http_userinfo(line: &str) -> std::borrow::Cow<'_, str> {
    let lower = line.to_ascii_lowercase();
    let mut masked = String::new();
    let mut copied = 0;
    for (separator, _) in lower.match_indices("://") {
        if !lower[..separator].ends_with("https") && !lower[..separator].ends_with("http") {
            continue;
        }
        let start = separator + 3;
        let end = line[start..]
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '`' | '<' | '>')
            })
            .map_or(line.len(), |offset| start + offset);
        if let Some(offset) = line[start..end].rfind('@') {
            let at = start + offset;
            masked.push_str(&line[copied..start]);
            masked.extend(line[start..at].chars().map(|_| '*'));
            copied = at;
        }
    }
    if copied == 0 {
        std::borrow::Cow::Borrowed(line)
    } else {
        masked.push_str(&line[copied..]);
        std::borrow::Cow::Owned(masked)
    }
}

fn ambiguous_expression(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut delimiters = Vec::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(delimiter) = quote {
            if !escaped && c == delimiter {
                quote = None;
            }
            escaped = !escaped && c == '\\';
        } else if matches!(c, '\'' | '"' | '`') {
            quote = Some(c);
        } else if c == '#' || (c == '/' && matches!(chars.peek(), Some('/' | '*'))) {
            return true;
        } else if matches!(c, '(' | '[' | '{') {
            delimiters.push(c);
        } else if let Some(open) = match c {
            ')' => Some('('),
            ']' => Some('['),
            '}' => Some('{'),
            _ => None,
        } {
            if delimiters.pop() != Some(open) {
                return true;
            }
        }
    }
    !delimiters.is_empty()
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
    #[test]
    fn masks_comment_secrets_without_excluding_safe_prose_or_embedded_prefixes() {
        let source = "// shouldn't keep a token here\nconst task_name = 'task-runner';\n";
        let safe = redact_source(source).unwrap();
        assert!(!safe.contains("shouldn't"));
        assert!(safe.ends_with("const task_name = 'task-runner';\n"));
        assert_eq!(
            redact_source("const task_name = 'task-runner';"),
            Some("const task_name = 'task-runner';".into())
        );
        assert!(redact_source("// token =\nunknown").is_none());
    }
    #[test]
    fn excludes_multiline_credential_continuations_before_excerpting() {
        for value in [
            "make(",
            "[",
            "{",
            "'first' +",
            "first &&",
            "first ||",
            "first ?",
            "first /",
            "first -",
            "first,",
            "first:",
            "first =",
            "['first'",
            "make('first'",
            "{ part: 'first'",
        ] {
            assert!(
                redact_source(&format!("const token = {value}\n'synthetic';\n")).is_none(),
                "{value}"
            );
        }
    }

    #[test]
    fn excludes_comment_hidden_credential_continuations_lf_and_crlf() {
        for newline in ["\n", "\r\n"] {
            for value in [
                "[ // parts",
                "[ /* parts */",
                "[ /* parts",
                "// parts",
                "[ # parts",
            ] {
                let raw = format!(
                    "const token = {value}{newline}'opaque-credential-value'{newline}];{newline}"
                );
                assert!(redact_source(&raw).is_none(), "{value}");
            }
        }
        let safe =
            redact_source("const token = 'https://opaque-credential-value/path#fragment';\n")
                .unwrap();
        assert!(!safe.contains("opaque-credential-value"));
        // A scheme-like object key is still followed by a real JS comment.
        assert!(redact_source(
            "const token = { http:// parts\nkey: 'opaque-credential-value'\n};\n"
        )
        .is_none());
    }

    #[test]
    fn excludes_next_line_credential_expressions_lf_and_crlf() {
        for newline in ["\n", "\r\n"] {
            for declaration in [
                "const token = ''",
                "const token",
                "token = ''",
                "token",
                "getConfig().token",
            ] {
                for continuation in [
                    "+ 'opaque-credential-value';",
                    "/* continuation */ + 'opaque-credential-value';",
                    "['opaque-credential-value'];",
                    "`opaque-credential-value`;",
                ] {
                    let raw = format!("{declaration}{newline}  {continuation}{newline}");
                    assert!(redact_source(&raw).is_none(), "{raw}");
                }
            }
            assert!(redact_source(&format!(
                "const token = '=>'{newline} + 'opaque-credential-value';{newline}"
            ))
            .is_none());
            for declaration in [
                "const token: (string) = ''",
                "getConfig().token = ''",
                "settings['('].token = ''",
            ] {
                assert!(redact_source(&format!(
                    "{declaration}{newline} + 'opaque-credential-value';{newline}"
                ))
                .is_none());
            }
        }
        assert!(redact_source("// token = useful prose\nconst ok = 1;\n").is_some());
        assert!(redact_source("const token = 'opaque';\nconst ok = 1;\n").is_some());
    }

    #[test]
    fn retains_authentication_functions_and_masks_http_url_userinfo() {
        for source in [
            "fn verify_password(password: &str) -> bool {\n    true\n}\n",
            "def verify_password(password):\n    return True\n",
            "function verify_password(password) {\n    return true;\n}\n",
            "const verify_password = (password) => {\n    return true;\n};\n",
        ] {
            assert!(redact_source(source).is_some(), "{source}");
        }
        for newline in ["\n", "\r\n"] {
            let raw = format!("const proxy = 'HTTP://alice:opaque-value@example.com:8080/path'; const other = \"https://bob:p%40ss@second.test?q=1#part\";{newline}const ok = 1;{newline}");
            let safe = redact_source(&raw).unwrap();
            assert!(!safe.contains("alice"));
            assert!(!safe.contains("opaque-value"));
            assert!(!safe.contains("p%40ss"));
            assert!(safe.contains("@example.com:8080/path"));
            assert!(safe.contains("@second.test?q=1#part"));
            assert!(safe.ends_with(&format!("{newline}const ok = 1;{newline}")));
            let safe = redact_source(
                "const proxy = \"https://o'reilly:opaque-value@example.com/path\";\n",
            )
            .unwrap();
            assert!(!safe.contains("opaque-value"));
            assert!(!safe.contains("o'reilly"));
            assert!(safe.contains("@example.com/path"));
        }
    }
}
