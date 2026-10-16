//! The docs must tell the truth about which client options are Rust-only.
//!
//! The list lives in the guide, and this check derives reality from code: the
//! core Client's public surface, minus whatever the binding layer reaches,
//! must equal the documented set. Bind a method without editing the docs, or
//! rename one without editing them, and the PR gate goes red.

/// The three files this check reads. Naming them in every problem is what
/// makes a red gate actionable.
pub const CORE_CLIENT: &str = "crates/simllms/src/client.rs";
pub const BINDINGS: &str = "crates/simllms-python/src/lib.rs";
pub const GUIDE: &str = "docs/getting-started.md";

const BLOCK_BEGIN: &str = "<!-- rust-only-begin -->";
const BLOCK_END: &str = "<!-- rust-only-end -->";

/// Surface names the binding layer reaches — those that occur as an identifier
/// token in its source. Token presence is a proxy, not a proof of a call; it
/// errs on the side of trusting the binding, so a genuine gap can only be
/// hidden by a name that appears in the binding for an unrelated reason.
fn bound_names(bindings: &str, surface: &[String]) -> Vec<String> {
    surface
        .iter()
        .filter(|name| identifiers(bindings).any(|token| token == name.as_str()))
        .cloned()
        .collect()
}

/// Backticked names inside the guide's marked block.
fn documented_names(guide: &str) -> Result<Vec<String>, String> {
    let begin = guide
        .find(BLOCK_BEGIN)
        .ok_or_else(|| format!("{GUIDE} has no `{BLOCK_BEGIN}` marker"))?;
    let rest = &guide[begin + BLOCK_BEGIN.len()..];
    let end = rest
        .find(BLOCK_END)
        .ok_or_else(|| format!("{GUIDE} opens `{BLOCK_BEGIN}` but never closes it"))?;
    Ok(backticked(&rest[..end]))
}

/// Name every way the guide's Rust-only list has departed from the code.
pub fn check(core_client: &str, bindings: &str, guide: &str) -> Vec<String> {
    let surface = public_surface(core_client);
    let bound = bound_names(bindings, &surface);
    let documented = match documented_names(guide) {
        Ok(names) => names,
        Err(problem) => return vec![problem],
    };

    let mut problems = Vec::new();
    for name in &surface {
        let is_bound = bound.contains(name);
        let is_documented = documented.contains(name);
        if is_bound && is_documented {
            problems.push(format!(
                "`{name}` is listed as Rust-only in {GUIDE} but {BINDINGS} now reaches it — \
                 remove it from the list"
            ));
        }
        if !is_bound && !is_documented {
            problems.push(format!(
                "`{name}` is reachable only from Rust but {GUIDE} does not say so — add it to \
                 the Rust-only list"
            ));
        }
    }
    for name in &documented {
        if !surface.contains(name) {
            problems.push(format!(
                "{GUIDE} lists `{name}` as Rust-only, but no such method exists on the core \
                 Client in {CORE_CLIENT} — the surface moved or the name is wrong"
            ));
        }
    }
    problems
}

/// Every method the core Client exposes to callers, in source order. Nothing
/// here filters by name shape: an unbound method is a gap whatever it is called.
///
/// Depth-tracked scan: only a `pub fn` at depth 1 of `impl Client` counts, so a
/// nested block never passes for the surface. Brace counting is blind to
/// comments and strings — the cost of reading source without a parser.
fn public_surface(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0usize;
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !inside {
            if trimmed == "impl Client {" {
                inside = true;
                depth = 1;
            }
            continue;
        }
        if depth == 1 {
            if let Some(name) = fn_name(trimmed) {
                names.push(name);
            }
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if depth == 0 {
            inside = false;
        }
    }
    names
}

fn fn_name(trimmed: &str) -> Option<String> {
    let rest = trimmed
        .strip_prefix("pub fn ")
        .or_else(|| trimmed.strip_prefix("pub async fn "))?;
    rest
        .split(['(', '<', ' '])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
}

fn backticked(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut pieces = text.split('`');
    // Odd-indexed pieces sit between a matched pair of backticks.
    let mut index = 0usize;
    while let Some(piece) = pieces.next() {
        if index % 2 == 1 {
            let name = piece.trim();
            if !name.is_empty() && identifiers(name).count() == 1 {
                names.push(name.to_string());
            }
        }
        index += 1;
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE: &str = "\
impl RetryPolicy {
    pub fn disabled() -> Self { todo!() }
}

impl Client {
    pub fn new(provider: Provider, model: &str, key: &str) -> Self { todo!() }
    pub fn with_base_url(&mut self, url: &str) -> Self { todo!() }
    pub fn with_retry(&mut self, policy: RetryPolicy) -> Self { todo!() }
    pub fn with_stream_timeouts(&mut self, first: Duration) -> Self { todo!() }
    pub async fn chat(&self, m: Vec<Message>) -> Reply { todo!() }
    pub fn refresh_models(&self) -> impl std::future::Future<Output = ()> { todo!() }
    pub fn warnings(&self) -> Vec<String> { todo!() }
}

impl Options {
    pub fn with_temperature(&mut self, t: f32) -> Self { todo!() }
}
";

    const GUIDE: &str = "\
Intro prose with `chat` in backticks outside the block.

<!-- rust-only-begin -->
- `with_retry` — 重试策略
- `with_stream_timeouts` — 流式超时
- `refresh_models` — 运行时刷新
- `warnings` — 结构化警告
<!-- rust-only-end -->

Trailing prose with `with_base_url` outside the block.
";

    // `new` is reached through the constructor; `chat` stands in for the
    // already-bound methods, which the fixture documents as nothing.
    const ALWAYS_BOUND: &[&str] = &["chat"];

    fn bindings_that_call(names: &[&str]) -> String {
        let mut text = "fn new() {\n    let client = simllms::Client::new(p, m, k);\n".to_string();
        for name in ALWAYS_BOUND.iter().chain(names.iter()) {
            text.push_str(&format!("    client = client.{name}(x);\n"));
        }
        text.push_str("}\n");
        text
    }

    #[test]
    fn surface_is_the_client_impls_whole_public_surface() {
        // No name-shape filter: `chat` is as much a surface member as
        // `with_retry`, so an unbound method is caught whatever it is called.
        assert_eq!(
            public_surface(CORE),
            vec![
                "new",
                "with_base_url",
                "with_retry",
                "with_stream_timeouts",
                "chat",
                "refresh_models",
                "warnings"
            ]
        );
    }

    #[test]
    fn only_bounded_names_leave_the_gap() {
        let surface = public_surface(CORE);
        let bound = bound_names(&bindings_that_call(&["with_base_url"]), &surface);
        assert_eq!(bound, vec!["new", "with_base_url", "chat"]);
    }

    #[test]
    fn a_substring_is_not_a_binding() {
        let surface = public_surface(CORE);
        // `with_base_url` appearing in prose-like text is still an identifier
        // token; a lookalike `with_base_urls` must not count as binding it.
        let bound = bound_names("let with_base_urls = 1;", &surface);
        assert!(bound.is_empty(), "{bound:?}");
    }

    #[test]
    fn docs_that_match_the_gap_pass() {
        let guide = format!(
            "\
<!-- rust-only-begin -->
- `with_retry`
- `with_stream_timeouts`
- `refresh_models`
- `warnings`
<!-- rust-only-end -->
"
        );
        assert_eq!(
            check(CORE, &bindings_that_call(&["with_base_url"]), &guide),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_undocumented_rust_only_option_is_named() {
        let guide = "<!-- rust-only-begin -->\n- `with_retry`\n<!-- rust-only-end -->\n";
        let problems = check(CORE, &bindings_that_call(&["with_base_url"]), guide);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems.iter().any(|p| p.contains("with_stream_timeouts")),
            "{problems:?}"
        );
        assert!(problems.iter().any(|p| p.contains("refresh_models")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("warnings")), "{problems:?}");
    }

    #[test]
    fn binding_a_method_without_editing_the_docs_fails() {
        let guide = "<!-- rust-only-begin -->\n- `with_retry`\n<!-- rust-only-end -->\n";
        // `with_retry` is now bound too, yet the guide still lists it; and the
        // other three remain Rust-only without being documented.
        let problems = check(CORE, &bindings_that_call(&["with_base_url", "with_retry"]), guide);
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(
            problems.iter().any(|p| p.contains("now reaches it")),
            "must say the list is stale: {problems:?}"
        );
    }

    #[test]
    fn a_name_that_no_longer_exists_fails() {
        let guide = "\
<!-- rust-only-begin -->
- `with_retry`
- `with_stream_timeouts`
- `refresh_models`
- `warnings`
- `with_temperature`
<!-- rust-only-end -->
";
        let problems = check(CORE, &bindings_that_call(&["with_base_url"]), guide);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("with_temperature"), "{problems:?}");
    }

    #[test]
    fn a_missing_marker_block_is_a_problem() {
        let problems = check(CORE, "", "no markers here\n");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains(BLOCK_BEGIN), "{problems:?}");
    }

    #[test]
    fn an_unclosed_marker_block_is_a_problem() {
        let problems = check(CORE, "", &format!("{BLOCK_BEGIN}\n- `with_retry`\n"));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("never closes"), "{problems:?}");
    }

    #[test]
    fn backticks_outside_the_block_are_ignored() {
        // `with_base_url` sits in trailing prose; the guide block is complete.
        let bindings = bindings_that_call(&["with_base_url"]);
        let guide = "\
<!-- rust-only-begin -->
- `with_retry`
- `with_stream_timeouts`
- `refresh_models`
- `warnings`
<!-- rust-only-end -->
";
        assert_eq!(check(CORE, &bindings, guide), Vec::<String>::new());
        assert_eq!(documented_names(GUIDE).unwrap().len(), 4);
    }

    #[test]
    fn multi_word_backtick_spans_are_not_names() {
        let names = backticked("`with_retry` and `not a name`");
        assert_eq!(names, vec!["with_retry"]);
    }
}
