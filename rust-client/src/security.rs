/// Remove connection credentials and common authentication values before
/// showing or recording diagnostic text returned by the core.
pub(crate) fn redact_sensitive_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let lower = rest.to_ascii_lowercase();
        let link_at = lower.find("vless://");
        let secret_at = find_secret_value(rest, &lower);
        let (at, value_start, quote, is_link) = match (link_at, secret_at) {
            (None, None) => {
                output.push_str(rest);
                break;
            }
            (Some(at), None) => (at, at + "vless://".len(), None, true),
            (None, Some((at, value_start, quote))) => (at, value_start, quote, false),
            (Some(link_at), Some((secret_at, _, _))) if link_at <= secret_at => {
                (link_at, link_at + "vless://".len(), None, true)
            }
            (Some(_), Some((at, value_start, quote))) => (at, value_start, quote, false),
        };
        output.push_str(&rest[..if is_link { at } else { value_start }]);
        if is_link {
            output.push_str("vless://[скрыто]");
            rest = rest[value_start..].trim_start_matches(|c: char| {
                !c.is_whitespace() && !matches!(c, '"' | '\'' | ')' | ']' | '}')
            });
            continue;
        }

        output.push_str("[скрыто]");
        let value = &rest[value_start..];
        let end = match quote {
            Some(quote) => quoted_value_end(value, quote),
            None => Some((unquoted_value_end(value), false)),
        };
        let Some((value_end, closing_quote)) = end else {
            // An unterminated quoted credential is hidden through end of input.
            rest = "";
            continue;
        };
        if closing_quote {
            output.push(quote.expect("quoted value has a quote character"));
        }
        rest = &value[value_end..];
    }
    output
}

fn find_secret_value(rest: &str, lower: &str) -> Option<(usize, usize, Option<char>)> {
    const KEYS: &[&str] = &[
        "password",
        "passwd",
        "pwd",
        "uuid",
        "pbk",
        "sid",
        "token",
        "secret",
        "private_key",
        "private-key",
        "api_key",
        "api-key",
        "username",
    ];

    let mut earliest = None;
    for key in KEYS {
        let mut search_from = 0;
        while let Some(relative) = lower[search_from..].find(key) {
            let at = search_from + relative;
            let end = at + key.len();
            let preceding_boundary = rest[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
            let following_boundary = rest[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
            if preceding_boundary && following_boundary {
                let mut cursor = end;
                // JSON and similar formats quote the key itself; keep the
                // closing quote when looking for the key/value separator.
                if rest[cursor..]
                    .chars()
                    .next()
                    .is_some_and(|c| matches!(c, '"' | '\''))
                {
                    cursor += rest[cursor..].chars().next().unwrap().len_utf8();
                }
                while rest[cursor..]
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace)
                {
                    cursor += rest[cursor..].chars().next().unwrap().len_utf8();
                }
                if rest[cursor..]
                    .chars()
                    .next()
                    .is_some_and(|c| matches!(c, '=' | ':'))
                {
                    cursor += 1;
                    while rest[cursor..]
                        .chars()
                        .next()
                        .is_some_and(char::is_whitespace)
                    {
                        cursor += rest[cursor..].chars().next().unwrap().len_utf8();
                    }
                    let quote = rest[cursor..]
                        .chars()
                        .next()
                        .filter(|c| matches!(c, '"' | '\''));
                    if let Some(quote) = quote {
                        cursor += quote.len_utf8();
                    }
                    if earliest.is_none_or(|(previous, _, _)| at < previous) {
                        earliest = Some((at, cursor, quote));
                    }
                    break;
                }
            }
            search_from = end;
        }
    }
    earliest
}

fn quoted_value_end(value: &str, quote: char) -> Option<(usize, bool)> {
    let mut escaped = false;
    for (at, c) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((at + c.len_utf8(), true));
        }
    }
    None
}

fn unquoted_value_end(value: &str) -> usize {
    value
        .char_indices()
        .find_map(|(at, c)| {
            (c.is_whitespace() || matches!(c, '&' | ',' | ';' | ')' | ']' | '}')).then_some(at)
        })
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use super::redact_sensitive_text;

    #[test]
    fn redacts_credentials_from_core_error_text() {
        let safe = redact_sensitive_text(
            "invalid remote vless://uuid-secret@example.org:443?pbk=public-key&sid=short-id token=api-secret",
        );
        assert!(!safe.contains("uuid-secret"));
        assert!(!safe.contains("public-key"));
        assert!(!safe.contains("short-id"));
        assert!(!safe.contains("api-secret"));
        assert!(safe.contains("vless://[скрыто]"));
        assert!(safe.contains("token=[скрыто]"));
    }

    #[test]
    fn redacts_json_credentials_and_query_parameters() {
        let safe = redact_sensitive_text(
            r#"{"Password" : "json-secret", "token" : "api-secret"} PBK=query-secret&sid=short-id"#,
        );
        for secret in ["json-secret", "api-secret", "query-secret", "short-id"] {
            assert!(!safe.contains(secret), "leaked {secret}: {safe}");
        }
        assert!(safe.contains(r#""Password" : "[скрыто]""#));
        assert!(safe.contains(r#""token" : "[скрыто]""#));
    }

    #[test]
    fn leaves_words_that_only_contain_a_secret_key_unchanged() {
        let text = "secretary=visible tokenized=visible";
        assert_eq!(redact_sensitive_text(text), text);
    }
}
