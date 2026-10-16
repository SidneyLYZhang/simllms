//! Authoritative Version: one place declares a Release's number, everything
//! else is a derived copy. The root workspace manifest is the source; the
//! binding crate's manifest is the only copy we write, because ADR-0001 keeps
//! it outside the root workspace and it therefore cannot inherit.

/// The layout this check knows: one source plus its derived copies. Naming the
/// file in every problem is what makes a red gate actionable.
pub const ROOT_MANIFEST: &str = "Cargo.toml";
pub const BINDING_MANIFEST: &str = "crates/simllms-python/Cargo.toml";
pub const PYPROJECT: &str = "crates/simllms-python/pyproject.toml";
pub const PYTHON_INIT: &str = "crates/simllms-python/python/simllms/__init__.py";

/// Validate the shape of a version we are about to propagate.
fn validate(version: &str) -> Result<(), String> {
    let core_and_extra = version.split_once(['-', '+']).map_or(version, |v| v.0);
    let parts: Vec<&str> = core_and_extra.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(format!(
            "`{version}` is not a `major.minor.patch` version — the Authoritative Version \
             must be three dot-separated numbers, optionally with a `-pre` or `+build` suffix"
        ));
    }
    Ok(())
}

/// Read `[workspace.package] version` out of the root manifest.
pub fn authoritative_version(root_manifest: &str) -> Result<String, String> {
    let found = quoted_in_section(root_manifest, "[workspace.package]", "version").ok_or_else(
        || {
            format!(
                "{ROOT_MANIFEST} has no `[workspace.package] version` — that line is the \
                 Authoritative Version and everything else derives from it"
            )
        },
    )?;
    validate(&found)?;
    Ok(found)
}

/// The binding crate's manifest with its `[package] version` set to `version`.
pub fn render_derived(binding_manifest: &str, version: &str) -> Result<String, String> {
    validate(version)?;
    if quoted_in_section(binding_manifest, "[package]", "version").is_none() {
        return Err(format!(
            "{BINDING_MANIFEST} has no `[package] version` line to derive into — it is the one \
             written copy of the Authoritative Version"
        ));
    }
    Ok(replace_quoted_in_section(
        binding_manifest,
        "[package]",
        "version",
        version,
    ))
}

/// Name every way the tree has drifted away from the Authoritative Version.
/// Empty means releasable. Pre-release suffixes are consistent here on purpose;
/// refusing an `rc` tag is the release line's job, not this check's.
pub fn inspect(
    root_manifest: &str,
    binding_manifest: &str,
    pyproject: &str,
    python_init: &str,
) -> Vec<String> {
    let mut problems = Vec::new();
    let version = match authoritative_version(root_manifest) {
        Ok(v) => v,
        Err(e) => {
            problems.push(e);
            return problems;
        }
    };

    match quoted_in_section(binding_manifest, "[package]", "version") {
        Some(v) if v == version => {}
        Some(v) => problems.push(format!(
            "{BINDING_MANIFEST} declares `{v}` but the Authoritative Version in {ROOT_MANIFEST} \
             is `{version}`; run `cargo xtask sync-version`"
        )),
        None => problems.push(format!(
            "{BINDING_MANIFEST} has no `[package] version` to derive"
        )),
    }

    if quoted_in_section(pyproject, "[project]", "version").is_some() {
        problems.push(format!(
            "{PYPROJECT} declares a static `version` — it must be derived, so remove it from \
             `[project]` and list it under `dynamic`"
        ));
    }
    let dynamic = array_in_section(pyproject, "[project]", "dynamic");
    if !dynamic.iter().any(|k| k == "version") {
        problems.push(format!(
            "{PYPROJECT} does not list `version` under `[project] dynamic`, so the build \
             backend would never read the derived version"
        ));
    }

    if let Some(line) = assignment_of(python_init, "__version__") {
        if line.contains('"') || line.contains('\'') {
            problems.push(format!(
                "{PYTHON_INIT} holds its own version literal — the reported version must come \
                 from the compiled extension module, so assign it from `_core.__version__`"
            ));
        }
    } else {
        problems.push(format!(
            "{PYTHON_INIT} no longer reports a version at all; it must re-export the extension \
             module's `__version__`"
        ));
    }

    problems
}

fn quoted_in_section(text: &str, section: &str, key: &str) -> Option<String> {
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
            return unquoted(rest);
        }
    }
    None
}

fn replace_quoted_in_section(text: &str, section: &str, key: &str, value: &str) -> String {
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

fn array_in_section(text: &str, section: &str, key: &str) -> Vec<String> {
    let raw = match quoted_line_value(text, section, key) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .filter_map(|item| match unquoted(item.trim()) {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        })
        .collect()
}

fn quoted_line_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section;
            continue;
        }
        if in_section {
            if let Some(rest) = stripped_key(trimmed, key) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn assignment_of<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        stripped_key(trimmed, name).map(|rest| rest.trim_start_matches('=').trim())
    })
}

/// `key = <rest>` inside a line, ignoring comments and `key_other`.
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

    const ROOT: &str = "[workspace]\nmembers = [\"crates/simllms\"]\n\n[workspace.package]\nversion = \"0.1.0\"\nlicense = \"MIT OR Apache-2.0\"\n";
    const BINDING: &str = "[package]\nname = \"simllms-python\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n";
    const PYPROJECT: &str = "[build-system]\nrequires = [\"maturin>=1.7,<2\"]\n\n[project]\nname = \"simllms\"\ndynamic = [\"version\"]\nrequires-python = \">=3.10\"\n";
    const INIT: &str = "from simllms import _core\n\n__version__ = _core.__version__\n\nLOGGER_NAME = \"simllms\"\n";

    fn with_binding_version(version: &str) -> String {
        BINDING.replace("version = \"0.1.0\"", &format!("version = \"{version}\""))
    }

    #[test]
    fn reads_the_workspace_version_not_a_package_version() {
        let text = "[package]\nversion = \"9.9.9\"\n\n[workspace.package]\nversion = \"0.1.0\"\n";
        assert_eq!(authoritative_version(text).unwrap(), "0.1.0");
    }

    #[test]
    fn rejects_versions_that_are_not_three_numbers() {
        for bad in ["0.1", "1.2.3.4", "", "abc", "0.1.x"] {
            assert!(
                validate(bad).is_err(),
                "`{bad}` must not be accepted as a version"
            );
        }
        for ok in ["0.1.0", "1.0.0", "0.2.0-rc.1", "1.0.0+build.7"] {
            assert!(validate(ok).is_ok(), "`{ok}` must be accepted");
        }
    }

    #[test]
    fn missing_authoritative_version_names_the_line_to_add() {
        let err = authoritative_version("[workspace]\nmembers = []\n").unwrap_err();
        assert!(err.contains("[workspace.package] version"), "{err}");
    }

    #[test]
    fn derives_only_the_binding_package_version() {
        let rendered = render_derived(BINDING, "0.2.0").unwrap();
        assert!(rendered.contains("version = \"0.2.0\""));
        assert!(rendered.contains("name = \"simllms-python\""));
        assert!(rendered.contains("[lib]"), "unrelated sections survive");
        // Idempotent: deriving from an already-derived copy changes nothing.
        assert_eq!(render_derived(&rendered, "0.2.0").unwrap(), rendered);
    }

    #[test]
    fn deriving_into_a_manifest_without_a_version_fails_loudly() {
        let err = render_derived("[package]\nname = \"x\"\n", "0.2.0").unwrap_err();
        assert!(err.contains("[package] version"), "{err}");
    }

    #[test]
    fn a_consistent_tree_reports_no_problems() {
        assert_eq!(inspect(ROOT, BINDING, PYPROJECT, INIT), Vec::<String>::new());
    }

    #[test]
    fn a_stale_derived_copy_is_named() {
        let problems = inspect(ROOT, &with_binding_version("0.0.9"), PYPROJECT, INIT);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("0.0.9") && problems[0].contains("0.1.0"));
        assert!(problems[0].contains("sync-version"), "must say how to fix it");
    }

    #[test]
    fn pre_release_versions_are_consistent_here() {
        let root = ROOT.replace("0.1.0", "0.2.0-rc.1");
        let binding = with_binding_version("0.2.0-rc.1");
        assert_eq!(
            inspect(&root, &binding, PYPROJECT, INIT),
            Vec::<String>::new(),
            "refusing an rc tag belongs to the release line, not to this check"
        );
    }

    #[test]
    fn a_static_python_version_is_a_problem() {
        let pyproject = PYPROJECT.replace("dynamic = [\"version\"]", "version = \"0.1.0\"");
        let problems = inspect(ROOT, BINDING, &pyproject, INIT);
        assert!(
            problems.iter().any(|p| p.contains("static")),
            "static pyproject version must fail: {problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("dynamic")),
            "the missing `dynamic` entry must also be named: {problems:?}"
        );
    }

    #[test]
    fn a_dynamic_list_without_version_is_a_problem() {
        let pyproject = PYPROJECT.replace("dynamic = [\"version\"]", "dynamic = [\"readme\"]");
        let problems = inspect(ROOT, BINDING, &pyproject, INIT);
        assert!(problems.iter().any(|p| p.contains("dynamic")), "{problems:?}");
    }

    #[test]
    fn a_version_literal_in_the_python_layer_is_a_problem() {
        let init = INIT.replace("__version__ = _core.__version__", "__version__ = \"0.1.0\"");
        let problems = inspect(ROOT, BINDING, PYPROJECT, &init);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("_core.__version__"), "{problems:?}");
    }

    #[test]
    fn a_missing_python_version_report_is_a_problem() {
        let problems = inspect(ROOT, BINDING, PYPROJECT, "LOGGER_NAME = \"simllms\"\n");
        assert!(
            problems.iter().any(|p| p.contains("no longer reports a version")),
            "{problems:?}"
        );
    }

    #[test]
    fn comments_do_not_count_as_a_static_version() {
        let pyproject = format!("{PYPROJECT}\n# version = \"0.1.0\"\n");
        assert_eq!(
            inspect(ROOT, BINDING, &pyproject, INIT),
            Vec::<String>::new(),
            "a commented-out version line is not a declaration"
        );
    }
}
