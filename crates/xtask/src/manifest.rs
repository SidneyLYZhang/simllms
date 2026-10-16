//! The slice of TOML both checks need: read one key's value out of one table,
//! and rewrite it. Deliberately not a parser — the manifests we inspect are
//! hand-written and shallow, and a dependency that could parse them fully would
//! also let a wrong-but-parseable declaration pass unnoticed.
//!
//! A section ends at the next `[header]` line; keys are matched on the exact
//! prefix plus `=`, so `license` never matches `license-files` and a commented
//! line never counts as a declaration.

/// The raw right-hand side of `key` inside `section`, before any comment or
/// quote is stripped, or `None` when the key is absent or commented out.
pub fn value_in_section<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(rest) = stripped_key(trimmed, key) {
            return Some(rest);
        }
    }
    None
}

/// A quoted scalar: `"value"` or `'value'`, with a trailing comment ignored.
pub fn quoted_in_section(text: &str, section: &str, key: &str) -> Option<String> {
    value_in_section(text, section, key).and_then(unquoted)
}

/// The members of an array: `key = ["a", "b"]`, whether it is written on one
/// line or spread over several. An absent key and an empty array both read as
/// no members; an unquoted member is not a member.
pub fn array_in_section(text: &str, section: &str, key: &str) -> Vec<String> {
    let Some(raw) = array_text(text, section, key) else {
        return Vec::new();
    };
    let inner = raw
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(|c| c != ']')
        .trim_end_matches(']');
    inner
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.starts_with('{') {
                // Poetry-style table entries — maturin's `include` uses them to
                // say which target a file belongs to. The entry *is* its path.
                let fields = item.trim_start_matches('{').trim_end_matches('}');
                return fields
                    .split(',')
                    .find_map(|field| stripped_key(field.trim(), "path").and_then(unquoted));
            }
            match unquoted(item) {
                Some(s) if !s.is_empty() => Some(s),
                _ => None,
            }
        })
        .collect()
}

fn array_text(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    let mut collected: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && collected.is_none() {
            in_section = trimmed == section;
            continue;
        }
        if !in_section {
            continue;
        }
        match &mut collected {
            Some(buf) => {
                buf.push(' ');
                buf.push_str(trimmed);
                if trimmed.contains(']') {
                    break;
                }
            }
            None => {
                let Some(rest) = stripped_key(trimmed, key) else {
                    continue;
                };
                if rest.contains(']') {
                    return Some(rest.to_string());
                }
                collected = Some(rest.to_string());
            }
        }
    }
    collected.filter(|raw| raw.contains(']'))
}

/// The first `key = <rest>` anywhere, ignoring which table it sits in. Used for
/// a Python assignment (`__version__ = …`), where there are no tables.
pub fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| stripped_key(line.trim(), key))
}

/// `section`'s `key` set to `value`, quoting it. Only the first declaration is
/// rewritten — a manifest that declares the same key twice is already a problem
/// for the check to name, not for the writer to guess about.
pub fn replace_quoted_in_section(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut in_section = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section;
            out.push_str(line);
            continue;
        }
        if in_section && stripped_key(trimmed, key).is_some() {
            out.push_str(&format!("{key} = \"{value}\"\n"));
            in_section = false;
            continue;
        }
        out.push_str(line);
    }
    out
}

/// `key = <rest>` inside a trimmed line, ignoring `key_other = …`.
fn stripped_key<'a>(trimmed: &'a str, key: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(key)?;
    let rest = rest.trim_start();
    if !rest.starts_with('=') {
        return None;
    }
    let rest = rest.trim_start_matches('=').trim();
    if rest.is_empty() || rest.starts_with('#') {
        return None;
    }
    Some(rest)
}

fn unquoted(value: &str) -> Option<String> {
    let value = value.split('#').next().unwrap_or("").trim();
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        return Some(value[1..value.len() - 1].to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
[package]
name = \"simllms\"
license = \"MIT OR Apache-2.0\"
keywords = [\"llm\", \"deepseek\"]

[dependencies]
license = \"ignored elsewhere\"
";

    #[test]
    fn reads_only_inside_the_named_section() {
        assert_eq!(
            quoted_in_section(TEXT, "[dependencies]", "license").as_deref(),
            Some("ignored elsewhere")
        );
        assert_eq!(
            quoted_in_section(
                "[workspace.package]\nversion = \"0.1.0\"\n",
                "[package]",
                "version"
            ),
            None
        );
    }

    #[test]
    fn a_key_prefix_never_matches_a_longer_key() {
        let text = "[project]\nlicense = { text = \"MIT\" }\nlicense-files = [\"LICENSE-MIT\"]\n";
        assert_eq!(
            quoted_in_section(text, "[project]", "license"),
            None,
            "an inline table is not a quoted scalar"
        );
        assert_eq!(
            array_in_section(text, "[project]", "license-files"),
            vec!["LICENSE-MIT".to_string()]
        );
    }

    #[test]
    fn comments_are_not_declarations() {
        let text = "[package]\n# version = \"0.0.9\"\n";
        assert_eq!(quoted_in_section(text, "[package]", "version"), None);
        assert_eq!(value_in_section(text, "[package]", "version"), None);
    }

    #[test]
    fn arrays_read_members_and_absent_keys_read_empty() {
        assert_eq!(
            array_in_section(TEXT, "[package]", "keywords"),
            vec!["llm".to_string(), "deepseek".to_string()]
        );
        assert_eq!(
            array_in_section(TEXT, "[package]", "categories"),
            Vec::<String>::new()
        );
        assert_eq!(
            array_in_section("[package]\nkeywords = []\n", "[package]", "keywords"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn trailing_comments_do_not_leak_into_a_quoted_value() {
        let text = "[package]\nlicense = \"MIT OR Apache-2.0\" # dual\n";
        assert_eq!(
            quoted_in_section(text, "[package]", "license").as_deref(),
            Some("MIT OR Apache-2.0")
        );
    }

    #[test]
    fn arrays_spread_over_several_lines_are_still_one_array() {
        let text = "\
[project]
classifiers = [
    \"Programming Language :: Rust\",
    \"License :: OSI Approved\",
]
keywords = [\"llm\"]
";
        assert_eq!(
            array_in_section(text, "[project]", "classifiers"),
            vec![
                "Programming Language :: Rust".to_string(),
                "License :: OSI Approved".to_string()
            ]
        );
        assert_eq!(
            array_in_section(text, "[project]", "keywords"),
            vec!["llm".to_string()],
            "the array after a multi-line one is still read"
        );
    }

    #[test]
    fn an_unterminated_array_reads_as_no_members() {
        let text = "[project]\nkeywords = [\n\"llm\"\n";
        assert_eq!(
            array_in_section(text, "[project]", "keywords"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_table_entry_reads_as_its_path() {
        let text = "\
[tool.maturin]
include = [
    { path = \"CHANGELOG.md\", format = \"sdist\" },
    { path = \"README.en.md\", format = \"sdist\" },
    \"LICENSE-MIT\",
]
";
        assert_eq!(
            array_in_section(text, "[tool.maturin]", "include"),
            vec![
                "CHANGELOG.md".to_string(),
                "README.en.md".to_string(),
                "LICENSE-MIT".to_string()
            ]
        );
    }
}
