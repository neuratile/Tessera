//! Conservative source redaction before review context assembly. Never changes lines.
//! This is not an authorization boundary: all surviving source remains untrusted.

const CREDENTIAL_PARTS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "tokens",
    "apikey",
    "authorization",
    "bearer",
];
const CREDENTIAL_COMPOUNDS: &[&str] = &[
    "tokenvalue",
    "passwordhash",
    "passwordvalue",
    "secretvalue",
    "secretkey",
    "apikeyvalue",
];
const TOKEN_METADATA_PARTS: &[&str] = &[
    "max",
    "min",
    "input",
    "output",
    "prompt",
    "completion",
    "total",
    "requested",
    "cached",
    "context",
    "available",
    "remaining",
    "used",
    "cache",
    "read",
    "write",
    "count",
    "counts",
    "limit",
    "budget",
    "usage",
    "length",
    "size",
];

/// Mask entire credential-bearing lines and PEM blocks, retaining CR/LF layout.
/// Reject unterminated PEM or ambiguous multiline credential values rather than
/// sending continuation lines whose boundaries cannot be established safely.
#[must_use]
pub fn redact_source(source: &str) -> Option<String> {
    let mut pem: Option<String> = None;
    let mut result = String::with_capacity(source.len());
    let mut lines = source.split_inclusive('\n');
    while let Some(raw_line) = lines.next() {
        let line = redact_url_userinfo(raw_line);
        let lower = line.to_ascii_lowercase();
        if let Some((_, rest)) = lower.split_once("-----begin ") {
            let (label, _) = rest.split_once("-----")?;
            if pem.is_some() || label.is_empty() {
                return None;
            }
            pem = Some(label.into());
        }
        let keyword = credential_term(&line);
        let opaque = lower.contains("bearer ")
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
        if pem.is_none()
            && (keyword || opaque)
            && (ambiguous_value(&line) || later_assignment(lines.clone()))
        {
            return None;
        }
        if pem.is_some() || opaque || (keyword && !reference_only(&line)) {
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

fn credential_term(text: &str) -> bool {
    let mut segmented = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if previous.is_some_and(|p| {
            (c.is_ascii_digit() && p.is_ascii_alphabetic())
                || (c.is_ascii_alphabetic() && p.is_ascii_digit())
                || (c.is_ascii_uppercase()
                    && (p.is_ascii_lowercase()
                        || (p.is_ascii_uppercase()
                            && chars.peek().is_some_and(char::is_ascii_lowercase))))
        }) {
            segmented.push('_');
        }
        segmented.push(c.to_ascii_lowercase());
        previous = Some(c);
    }
    segmented
        .split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-'))
        .any(credential_identifier)
}

fn credential_identifier(identifier: &str) -> bool {
    let parts = identifier.split(['_', '-']).filter(|s| !s.is_empty());
    let token_word = parts.clone().any(|word| matches!(word, "token" | "tokens"));
    let count_word = parts.clone().any(|word| {
        [
            "count", "counts", "limit", "budget", "usage", "length", "size",
        ]
        .contains(&word)
    });
    let plural = parts.clone().any(|word| word == "tokens");
    let role = parts
        .clone()
        .any(|word| TOKEN_METADATA_PARTS.contains(&word));
    if token_word
        && role
        && (plural || count_word)
        && parts.clone().all(|word| {
            matches!(word, "token" | "tokens")
                || TOKEN_METADATA_PARTS.contains(&word)
                || word.chars().all(|c| c.is_ascii_digit())
        })
    {
        return false;
    }
    let compact = identifier.replace(['_', '-'], "");
    if CREDENTIAL_PARTS
        .iter()
        .any(|suffix| compact.ends_with(suffix))
        || CREDENTIAL_COMPOUNDS.contains(&compact.as_str())
    {
        return true;
    }
    let mut prior = "";
    for word in parts {
        if CREDENTIAL_PARTS.contains(&word) || (prior == "api" && word == "key") {
            return true;
        }
        prior = word;
    }
    false
}

fn value_declaration(line: &str) -> Option<(&str, &str)> {
    let line = without_safe_comment(line);
    line.split_once('=')
        .filter(|(target, value)| {
            !target.trim_end().ends_with(['!', '<', '>', '=']) && !value.starts_with(['=', '>'])
        })
        .or_else(|| {
            line.split_once(':').filter(|(target, value)| {
                !target.contains('(')
                    && !target.trim_start().starts_with("class ")
                    && !target.ends_with(':')
                    && !value.starts_with(':')
            })
        })
}

fn reference_only(line: &str) -> bool {
    // Only retain code identifiers, never literal data or comment contents.
    // Boundary checks still run first, including qualified/split assignments.
    let line = without_safe_comment(line);
    if line.contains(['\'', '"', '`', '#', '/'])
        || line.contains("/*")
        || value_declaration(line).is_some_and(|(target, _)| credential_term(target))
    {
        return false;
    }
    let trimmed = line.trim();
    let declaration = if trimmed.starts_with("pub(") {
        trimmed.split_once(") ").map_or(trimmed, |(_, tail)| tail)
    } else {
        trimmed.strip_prefix("pub ").unwrap_or(trimmed)
    };
    let identifier = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
    };
    let name = trimmed.trim_end_matches([',', ';']);
    line.contains("::")
        || identifier(name)
        || ["use ", "mod "].iter().any(|prefix| {
            declaration
                .strip_prefix(prefix)
                .is_some_and(|tail| identifier(tail.trim_end_matches([';', '{']).trim()))
        })
        || (!line.contains('=')
            && [
                "fn ",
                "async fn ",
                "def ",
                "async def ",
                "function ",
                "async function ",
            ]
            .iter()
            .any(|prefix| declaration.starts_with(prefix)))
}

fn without_safe_comment(line: &str) -> &str {
    if let Some((code, comment)) = line.split_once("//") {
        if !code.contains(['\'', '"', '`']) && !credential_term(comment) {
            return code;
        }
    }
    line
}

fn later_assignment<'a>(lines: impl Iterator<Item = &'a str>) -> bool {
    let mut block_comment = false;
    for (index, raw) in lines.enumerate() {
        // ponytail: bound trivia scanning to 64 lines; omit longer uncertain
        // continuations instead of introducing a cross-language parser.
        if index == 64 {
            return true;
        }
        let mut rest = trim_continuation(raw);
        loop {
            if block_comment {
                let Some((head, tail)) = rest.split_once("*/") else {
                    if rest.contains("/*") {
                        return true;
                    }
                    break;
                };
                if head.contains("/*") {
                    // Nested block comments differ across languages; exclude
                    // uncertain source rather than mistake an inner close.
                    return true;
                }
                rest = tail;
                block_comment = false;
            }
            rest = rest.trim_start_matches(|c: char| {
                c.is_whitespace() || matches!(c, '\u{feff}' | ']' | ')' | '}')
            });
            if rest.is_empty() {
                break;
            }
            if rest.starts_with("//") || rest.starts_with('#') {
                // JS line comments can end before LF at CR, U+2028 or U+2029.
                if let Some((_, tail)) = rest.split_once(['\r', '\u{2028}', '\u{2029}']) {
                    rest = tail;
                    continue;
                }
                break;
            }
            if let Some(tail) = rest.strip_prefix("/*") {
                rest = tail;
                block_comment = true;
                continue;
            }
            // Operators, member access, casts, and tagged templates may
            // continue the sensitive expression before its eventual value.
            // Exclude that uncertainty rather than stop before an assignment.
            if rest.starts_with(['.', '[', '(', '!', '`', ','])
                || ["as", "satisfies", "in", "instanceof"]
                    .iter()
                    .any(|keyword| {
                        rest.strip_prefix(keyword).is_some_and(|tail| {
                            tail.is_empty()
                                || tail.starts_with(|c: char| {
                                    !c.is_alphanumeric() && c != '_' && c != '$'
                                })
                        })
                    })
            {
                return true;
            }
            let Some((operator, value)) = rest.split_once('=') else {
                return false;
            };
            return [
                "", "+", "-", "*", "/", "%", "**", "&&", "||", "??", "<<", ">>", ">>>", "&", "|",
                "^",
            ]
            .contains(&operator.trim())
                && !value.starts_with(['=', '>']);
        }
    }
    block_comment
}

fn trim_continuation(text: &str) -> &str {
    // ECMAScript also accepts BOM as whitespace, beyond Rust's White_Space set.
    text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn ambiguous_value(line: &str) -> bool {
    let trimmed = line.trim_start();
    let comment = ["//", "#", "/*", "*", "<!--"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let declaration = value_declaration(line);
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
        // An unfinished parameter list can introduce a default on a later
        // line, even when this line has no '=' yet. Complete signatures remain.
        return !comment && line.matches('(').count() > line.matches(')').count();
    }
    // Deliberately exclude ambiguous declarations instead of parsing arbitrary
    // languages. This also covers template literals and YAML block scalars.
    if let Some((_, value)) = declaration {
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
    if comment {
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

fn redact_url_userinfo(line: &str) -> std::borrow::Cow<'_, str> {
    let mut masked = String::new();
    let mut copied = 0;
    // Authority-shaped userinfo is sensitive regardless of scheme semantics.
    for (separator, _) in line.match_indices("://") {
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
    fn retains_token_metadata_and_rust_references_without_retaining_values() {
        let raw = "use crate::utils::secret_redaction; // max_tokens=6000\npub(crate) use secret_redaction;\nmod secret_redaction;\nuse crate::{\n secret_redaction,\n};\nconst MAX_TOKENS: usize = 6000;\nlet input_tokens = 12;\nlet outputTokens = 40;\nlet reference = crate::secret_redaction::redact_source;\n";
        assert_eq!(redact_source(raw).as_deref(), Some(raw));
        for name in [
            "accessToken",
            "auth_token",
            "API_KEY",
            "apiKey",
            "secret_value",
            "clientSecret",
            "passwordHash",
            "access_tokens",
            "refreshTokens",
            "auth_tokens",
            "tokens",
            "mytoken",
            "mytokens",
            "token1",
            "password2",
            "TOKENvalue",
            "passwordhash",
        ] {
            let raw = format!("const {name} = 'opaque-credential-value';\n");
            assert!(
                !redact_source(&raw)
                    .unwrap()
                    .contains("opaque-credential-value"),
                "{name}"
            );
        }
        assert!(redact_source("crate::TOKEN\n = \"opaque-credential-value\";\n").is_none());
        assert!(
            redact_source("function max(tokens =\n 'opaque-credential-value'\n) {}\n").is_none()
        );
        let metadata =
            "const max_context_tokens = 8000;\nlet tokenCount = 12;\nlet maxTokens2 = 40;\n";
        assert_eq!(redact_source(metadata).as_deref(), Some(metadata));
        let raw = "use crate::secret_redaction; // token = opaque-credential-value;\n";
        assert!(!redact_source(raw)
            .unwrap()
            .contains("opaque-credential-value"));
        let regex = "const pattern = /token::opaque-credential-value/;\n";
        assert!(!redact_source(regex)
            .unwrap()
            .contains("opaque-credential-value"));
    }

    #[test]
    fn masks_authority_userinfo_for_all_schemes_and_retains_host_only_urls() {
        for scheme in [
            "http",
            "https",
            "postgres",
            "postgresql",
            "mysql",
            "mongodb",
            "mongodb+srv",
            "redis",
            "amqp",
            "custom+v1",
        ] {
            for newline in ["\n", "\r\n"] {
                let raw = format!("const uri = '{scheme}://alice:opaque-credential-value@example.test:1234/path?ok=1#part';{newline}");
                let safe = redact_source(&raw).unwrap();
                assert!(!safe.contains("alice"));
                assert!(!safe.contains("opaque-credential-value"));
                assert!(safe.contains(&format!("{scheme}://")));
                assert!(safe.contains("@example.test:1234/path?ok=1#part"));
                assert!(safe.ends_with(newline));
                let raw = format!("const uri = '{scheme}://example.test/path';{newline}");
                assert_eq!(redact_source(&raw).as_deref(), Some(raw.as_str()));
            }
        }
    }
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
        for prefix in ["ghp_", "gho_", "xoxb-", "glpat-", "AKIA", "AIza"] {
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

    #[test]
    fn excludes_function_values_and_multiline_default_credentials() {
        for newline in ["\n", "\r\n"] {
            for raw in [
                "const token = (() => {\n  return 'opaque-credential-value';\n})();\n",
                "const token = functionFromParts(\n  'opaque-credential-value'\n);\n",
                "const token = functionFromParts('first')\n  + 'opaque-credential-value';\n",
                "function connect(password =\n  'opaque-credential-value'\n) {}\n",
                "function connect(password:\n  string =\n  'opaque-credential-value'\n) {}\n",
                "def connect(password =\n  'opaque-credential-value'\n):\n    pass\n",
                "const verify_password = (password) => {\n  return true;\n};\n",
            ] {
                assert!(
                    redact_source(&raw.replace('\n', newline)).is_none(),
                    "{raw}"
                );
            }
        }
    }

    #[test]
    fn excludes_split_computed_assignments_across_trivia_and_closing_brackets() {
        for newline in ["\n", "\r\n"] {
            for raw in [
                "settings['token']\n= 'opaque-credential-value';\n",
                "(settings[\n 'token'\n ] )\n = 'opaque-credential-value';\n",
                "(settings[\n 'token'\n ] /* parts */ )\n = 'opaque-credential-value';\n",
                "settings[\n 'token'\n ]\n .value\n = 'opaque-credential-value';\n",
                "(settings[\n 'token'\n ] as string)\n = 'opaque-credential-value';\n",
                "settings['token'] //;\n= 'opaque-credential-value';\n",
                "settings['token'] //{\n= 'opaque-credential-value';\n",
                "settings['token']\n \u{feff}= 'opaque-credential-value';\n",
                "settings['token']\n // details\u{2028}= 'opaque-credential-value';\n",
                "settings['token']\n // details\u{2029}= 'opaque-credential-value';\n",
                "settings['token']\n // details\r= 'opaque-credential-value';\n",
                "// details\u{2028}settings['token']\n= 'opaque-credential-value';\n",
                "/* details */ settings['token']\n= 'opaque-credential-value';\n",
                "class Credentials {\n #token\n = 'opaque-credential-value';\n}\n",
                "*access_token\n = \"opaque-credential-value\";\n",
                "settings['token']\n // parts\n /* details */ = 'opaque-credential-value';\n",
                "settings['token']\n /* multiple\n lines */ = 'opaque-credential-value';\n",
                "settings[\"token\"]\n /* outer /* inner */ outer */ = \"opaque-credential-value\";\n",
                "settings[\n 'token'\n ]\n= 'opaque-credential-value';\n",
                "settings['token']\n??= 'opaque-credential-value';\n",
            ] {
                assert!(
                    redact_source(&raw.replace('\n', newline)).is_none(),
                    "{raw}"
                );
            }
        }
        let long = format!(
            "settings['token']\n{}= 'opaque-credential-value';\n",
            "// parts\n".repeat(65)
        );
        assert!(redact_source(&long).is_none());
        assert!(redact_source(
            "function verify_password(password) {\n // details\n return true;\n}\n"
        )
        .is_some());
    }
}
