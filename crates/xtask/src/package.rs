//! What has to be inside a published artifact, and the copies that put it there.
//!
//! Cargo's `include` and maturin's are both scoped to the package directory: a
//! root-level file cannot be packed by pointing at it, because cargo drops paths
//! outside the crate directory without complaint and maturin refuses them as
//! "resolves outside allowed metadata root". So each package carries a *derived
//! copy*, exactly as the binding manifest carries a derived copy of the
//! Authoritative Version — `sync-package` writes them, and a check rather than a
//! habit is what keeps them equal to the file at the root.

use crate::manifest::{array_in_section, quoted_in_section, value_in_section};

/// Root files that every released package has to carry.
pub const CARRIED_FILES: &[&str] = &[
    "CHANGELOG.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "README.en.md",
    "THIRD-PARTY-NOTICES.md",
];

/// The subset that travels as licensing metadata rather than as documentation:
/// PEP 639 puts every one of these under `*.dist-info/licenses/`, which is where
/// an installed distribution keeps the notices its licenses point at.
const LICENSE_FILES: &[&str] = &["LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md"];

/// The package layout, named once: both checks — and the version derivation —
/// read these paths, so adding a package should only mean editing here.
pub const CRATE_DIR: &str = "crates/simllms";
pub const CRATE_MANIFEST: &str = "crates/simllms/Cargo.toml";
pub const PYTHON_DIR: &str = "crates/simllms-python";
pub const PYPROJECT: &str = "crates/simllms-python/pyproject.toml";
pub const BINDING_MANIFEST: &str = "crates/simllms-python/Cargo.toml";
pub const PYTHON_INIT: &str = "crates/simllms-python/python/simllms/__init__.py";

/// What the crate's own source tree has to contribute to the package.
const CRATE_SOURCE_DIRS: &[&str] = &["src", "examples"];

/// Files cargo puts in a `.crate` whatever the manifest asks for.
const CARGO_FORCED: &[&str] = &[
    ".cargo_vcs_info.json",
    "Cargo.lock",
    "Cargo.toml",
    "Cargo.toml.orig",
];

/// What both registries must show as the license. The crate inherits it from
/// `[workspace.package]`, so the declaration lives in the root manifest and the
/// check reads both.
pub const LICENSE: &str = "MIT OR Apache-2.0";

/// The keyword that carries the claim the library really makes: it speaks the
/// OpenAI wire format, so any compatible gateway is in scope.
pub const CROSS_PROVIDER_KEYWORD: &str = "openai-compatible";

/// Provider names must stay out of the summary — they belong in keywords, where
/// they are searchable, and in the README, where they can be qualified.
const PROVIDER_NAMES: &[&str] = &[
    "deepseek",
    "moonshot",
    "kimi",
    "qwen",
    "dashscope",
    "alibaba",
    "zhipu",
    "chatglm",
    "xiaomi",
    "mimo",
    "openrouter",
];

/// crates.io takes at most five keywords, each of at most twenty characters.
/// PyPI has no such cap, so the crate is the side that has to choose.
const CRATE_KEYWORD_LIMIT: usize = 5;
const KEYWORD_CHAR_LIMIT: usize = 20;

/// The root file and the copy that must mirror it, for both packages.
pub fn copy_pairs() -> Vec<(&'static str, String)> {
    CARRIED_FILES
        .iter()
        .flat_map(|file| [(CRATE_DIR, *file), (PYTHON_DIR, *file)])
        .map(|(dir, file)| (file, format!("{dir}/{file}")))
        .collect()
}

/// One derived copy, read from both ends: the file at the root and whatever is
/// on disk where the package needs it.
pub struct CopyRead<'a> {
    pub root: &'a str,
    pub dest: &'a str,
    pub root_text: &'a str,
    pub copy_text: Option<&'a str>,
}

/// Every copy judged against the file it derives from.
pub fn copy_problems(copies: &[CopyRead<'_>]) -> Vec<String> {
    copies
        .iter()
        .filter_map(|copy| match copy.copy_text {
            None => Some(format!(
                "{} is missing: it is the copy of the repository-root {} that this package has \
                 to carry; run `cargo xtask sync-package`",
                copy.dest, copy.root
            )),
            Some(text) if text != copy.root_text => Some(format!(
                "{} does not match the repository-root {}; run `cargo xtask sync-package` to \
                 rewrite it",
                copy.dest, copy.root
            )),
            Some(_) => None,
        })
        .collect()
}

/// `crate::manifest` helpers are per-table, so a whole-manifest rule reads the
/// tables one at a time and reports every way it falls short. `root_manifest`
/// is where an inherited `license` is actually declared.
pub fn check_crate_manifest(text: &str, root_manifest: &str) -> Vec<String> {
    let mut problems = Vec::new();

    let include = array_in_section(text, "[package]", "include");
    if include.is_empty() {
        problems.push(format!(
            "{CRATE_MANIFEST} has no `[package] include` list — a published crate states what it \
             carries, so that packing the wrong thing goes red instead of letting extra files \
             out. List `{CRATE_SOURCE_DIRS:?}` and {CARRIED_FILES:?} there"
        ));
    } else {
        for required in CRATE_SOURCE_DIRS {
            if !covers(&include, required) {
                problems.push(format!(
                    "{CRATE_MANIFEST} does not list `{required}` under `[package] include`, so \
                     {CRATE_DIR}/{required} would be left out of the crate"
                ));
            }
        }
        for file in uncovered_carried(&include) {
            problems.push(format!(
                "{CRATE_MANIFEST} does not list `{file}` under `[package] include`, so a caller \
                 could not find it in the crate; run `cargo xtask sync-package` and add the entry"
            ));
        }
    }

    problems.extend(check_readme(text, "[package]", CRATE_MANIFEST));
    problems.extend(check_description(
        quoted_in_section(text, "[package]", "description").as_deref(),
        CRATE_MANIFEST,
    ));

    match quoted_in_section(text, "[package]", "license").as_deref() {
        Some(LICENSE) => {}
        Some(other) => problems.push(format!(
            "{CRATE_MANIFEST} declares `{other}` — both registries must show the same dual \
             license, spelled `{LICENSE}`"
        )),
        None if value_in_section(text, "[package]", "license.workspace") == Some("true") => {
            match quoted_in_section(root_manifest, "[workspace.package]", "license").as_deref() {
                Some(LICENSE) => {}
                Some(other) => problems.push(format!(
                    "Cargo.toml's `[workspace.package] license` is `{other}`, which \
                     {CRATE_MANIFEST} inherits — both registries must show `{LICENSE}`, so write \
                     `license = \"{LICENSE}\"` in Cargo.toml"
                )),
                None => problems.push(format!(
                    "Cargo.toml has no `[workspace.package] license`, so {CRATE_MANIFEST} \
                     inherits nothing and the crate declares no license at all; write \
                     `license = \"{LICENSE}\"` in Cargo.toml"
                )),
            }
        }
        None => problems.push(format!(
            "{CRATE_MANIFEST} declares no `license`; crates.io refuses a package with neither \
             `license` nor `license-file` — write `license = \"{LICENSE}\"`"
        )),
    }
    if value_in_section(text, "[package]", "license-file").is_some() {
        problems.push(format!(
            "{CRATE_MANIFEST} sets both `license` and `license-file`, which cargo rejects — the \
             SPDX expression is the declaration, and the license files travel in `include`"
        ));
    }

    let keywords = array_in_section(text, "[package]", "keywords");
    if keywords.len() > CRATE_KEYWORD_LIMIT {
        problems.push(format!(
            "{CRATE_MANIFEST} lists {} keywords but crates.io takes at most \
             {CRATE_KEYWORD_LIMIT} — PyPI has no cap, so drop the ones {PYPROJECT} does not need",
            keywords.len()
        ));
    }
    for keyword in &keywords {
        if let Some(reason) = illegal_crate_keyword(keyword) {
            problems.push(format!(
                "{CRATE_MANIFEST} has the keyword `{keyword}`: {reason}"
            ));
        }
    }
    if !keywords.iter().any(|k| k == CROSS_PROVIDER_KEYWORD) {
        problems.push(format!(
            "{CRATE_MANIFEST} is missing the `{CROSS_PROVIDER_KEYWORD}` keyword under \
             `[package] keywords` — the cross-provider claim is what a caller searches for"
        ));
    }

    problems
}

/// The Python distribution's own metadata.
pub fn check_pyproject(text: &str) -> Vec<String> {
    let mut problems = Vec::new();

    match value_in_section(text, "[project]", "license") {
        Some(raw) if raw.trim_start().starts_with('{') => problems.push(format!(
            "{PYPROJECT} states its license as a table (`license = {{ … }}`), which PEP 639 \
             deprecated: use the SPDX expression `license = \"{LICENSE}\"` and list the files \
             under `license-files`"
        )),
        Some(_) if quoted_in_section(text, "[project]", "license").as_deref() != Some(LICENSE) => {
            problems.push(format!(
                "{PYPROJECT} declares a license other than `{LICENSE}` — PyPI must show the same \
                 dual license the crate does"
            ))
        }
        Some(_) => {}
        None => problems.push(format!(
            "{PYPROJECT} declares no `license`; PyPI shows no license at all, and the license \
             files have nowhere to travel from — write `license = \"{LICENSE}\"`"
        )),
    }

    let license_files = array_in_section(text, "[project]", "license-files");
    for required in LICENSE_FILES {
        if !license_files.iter().any(|f| f == required) {
            problems.push(format!(
                "{PYPROJECT} does not list `{required}` under `[project] license-files`, so it \
                 would not reach an installed distribution"
            ));
        }
    }

    for pattern in &license_files {
        if pattern.contains("..") {
            problems.push(format!(
                "{PYPROJECT} lists `{pattern}` under `[project] license-files` — PEP 639 forbids \
                 `..` there and the build backend refuses the build, which is why each package \
                 keeps a copy of the files it carries"
            ));
        }
    }

    problems.extend(check_readme(text, "[project]", PYPROJECT));
    problems.extend(check_description(
        quoted_in_section(text, "[project]", "description").as_deref(),
        PYPROJECT,
    ));

    for classifier in array_in_section(text, "[project]", "classifiers") {
        if classifier.starts_with("License ::") {
            problems.push(format!(
                "{PYPROJECT} keeps the `{classifier}` classifier — PEP 639 forbids `License ::` \
                 alongside an SPDX expression, and PyPI rejects the upload"
            ));
        }
    }

    let keywords = array_in_section(text, "[project]", "keywords");
    if !keywords.iter().any(|k| k == CROSS_PROVIDER_KEYWORD) {
        problems.push(format!(
            "{PYPROJECT} is missing the `{CROSS_PROVIDER_KEYWORD}` keyword — the cross-provider \
             claim is what a caller searches for"
        ));
    }

    let include = array_in_section(text, "[tool.maturin]", "include");
    // maturin packs the module tree (`python/` plus the compiled extension) by
    // itself; this list is for what lives beside it, which is the carried files.
    if include.is_empty() {
        problems.push(format!(
            "{PYPROJECT} has no `[tool.maturin] include` list, so the source distribution \
             states nothing about what it carries — list {CARRIED_FILES:?} there with \
             `format = \"sdist\"`"
        ));
    } else {
        for file in uncovered_carried(&include) {
            problems.push(format!(
                "{PYPROJECT} does not list `{file}` under `[tool.maturin] include`, so the \
                 source distribution would not carry it"
            ));
        }
    }

    problems
}

/// The two sides have to say the same things about the same project.
pub fn check_agreement(crate_manifest: &str, pyproject: &str) -> Vec<String> {
    let mut problems = Vec::new();

    let crate_description = quoted_in_section(crate_manifest, "[package]", "description");
    let python_description = quoted_in_section(pyproject, "[project]", "description");
    if crate_description != python_description {
        problems.push(format!(
            "{} and {PYPROJECT} describe the project differently ({:?} vs {:?}) — one sentence, \
             written once, shown on both registries",
            CRATE_MANIFEST, crate_description, python_description
        ));
    }

    let crate_keywords = array_in_section(crate_manifest, "[package]", "keywords");
    let python_keywords = array_in_section(pyproject, "[project]", "keywords");
    for keyword in &crate_keywords {
        if !python_keywords.iter().any(|k| k == keyword) {
            problems.push(format!(
                "{CRATE_MANIFEST} keywords `{keyword}` but {PYPROJECT} does not — PyPI has no \
                 five-keyword cap, so it carries every keyword the crate can"
            ));
        }
    }

    problems
}

/// The file list cargo reports for the packed crate, judged against the
/// whitelist that produced it. `listing` is `cargo package --list` output —
/// which uses `\` on Windows and `/` elsewhere, while the archive itself is a
/// tar and therefore always `/`, so separators are normalized before judging.
pub fn check_crate_listing(listing: &str, crate_manifest: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let include = array_in_section(crate_manifest, "[package]", "include");
    let packed: Vec<String> = listing
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect();
    let packed: Vec<&str> = packed.iter().map(String::as_str).collect();

    for required in CARRIED_FILES {
        if !packed.iter().any(|p| p == required) {
            problems.push(format!(
                "the packed crate has no `{required}`: {CRATE_DIR} must hold the copy that \
                 `[package] include` asks for; run `cargo xtask sync-package`"
            ));
        }
    }
    for dir in CRATE_SOURCE_DIRS {
        if !packed.iter().any(|p| p.starts_with(&format!("{dir}/"))) {
            problems.push(format!(
                "the packed crate has nothing under `{dir}/`, although {CRATE_MANIFEST} lists it: \
                 {CRATE_DIR}/{dir} is missing or empty"
            ));
        }
    }

    for path in &packed {
        let top = path.split('/').next().unwrap_or(*path);
        if CARGO_FORCED.contains(path) || covers(&include, path) {
            continue;
        }
        problems.push(format!(
            "the packed crate carries `{path}`, which {CRATE_MANIFEST} never asked for — the \
             top-level entry `{top}` belongs in `[package] include` or out of {CRATE_DIR}"
        ));
    }

    problems
}

/// Does a whitelist entry bring `path` along? A directory entry covers its
/// contents, and a `/**` suffix says the same thing louder.
fn covers(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches("/**").trim_end_matches('/');
        pattern == path || path.starts_with(&format!("{pattern}/"))
    })
}

/// The carried files a whitelist still leaves out — the rule both manifests
/// share, even though each names its own table in the message.
fn uncovered_carried(include: &[String]) -> Vec<&'static str> {
    CARRIED_FILES
        .iter()
        .copied()
        .filter(|file| !covers(include, file))
        .collect()
}

fn check_readme(text: &str, section: &str, manifest: &str) -> Vec<String> {
    match quoted_in_section(text, section, "readme").as_deref() {
        // The registry page renders this file, and the repository's default
        // README is the Chinese one; only the English copy belongs in a package.
        Some("README.en.md") => Vec::new(),
        Some(other) => vec![format!(
            "{manifest} points `readme` at `{other}` — a registry page should render the English \
             README, which is `README.en.md`"
        )],
        None => vec![format!(
            "{manifest} declares no `readme`, so the registry page renders nothing (or the \
             repository's Chinese README, if a copy of it is packed) — write `readme = \
             \"README.en.md\"`"
        )],
    }
}

fn check_description(value: Option<&str>, manifest: &str) -> Vec<String> {
    let Some(value) = value else {
        return vec![format!(
            "{manifest} declares no `description`, so the registry summary line is empty"
        )];
    };
    let mut problems = Vec::new();
    let lower = value.to_lowercase();
    if let Some(provider) = PROVIDER_NAMES.iter().find(|p| lower.contains(**p)) {
        problems.push(format!(
            "{manifest}'s description names `{provider}` — keep it one provider-agnostic \
             sentence and list providers in the README and the keywords"
        ));
    }
    if value.split('.').filter(|s| !s.trim().is_empty()).count() > 1 || value.contains('\n') {
        problems.push(format!(
            "{manifest}'s description is more than one sentence — the summary line is a \
             headline, not an abstract"
        ));
    }
    problems
}

fn illegal_crate_keyword(keyword: &str) -> Option<String> {
    if keyword.is_empty() {
        return Some("a keyword is not empty".to_string());
    }
    if keyword.len() > KEYWORD_CHAR_LIMIT {
        return Some(format!(
            "crates.io keywords are at most {KEYWORD_CHAR_LIMIT} characters"
        ));
    }
    if !keyword
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return Some("crates.io keywords start with a letter or digit".to_string());
    }
    if !keyword
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
    {
        return Some("crates.io keywords use letters, digits, `_`, `-` or `+` only".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTION: &str = "Unified client for multiple LLM providers";
    const ROOT: &str = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nlicense = \"MIT OR Apache-2.0\"\n";

    /// The real crate inherits its license, so every crate-manifest rule needs
    /// the root manifest too.
    fn crate_problems(manifest: &str) -> Vec<String> {
        check_crate_manifest(manifest, ROOT)
    }

    fn crate_manifest(include: &str, keywords: &str) -> String {
        format!(
            "[package]\nname = \"simllms\"\ndescription = \"{DESCRIPTION}\"\n\
             license = \"MIT OR Apache-2.0\"\nreadme = \"README.en.md\"\n\
             keywords = [{keywords}]\ninclude = [{include}]\n"
        )
    }

    fn carried_include() -> String {
        let mut entries: Vec<String> = CRATE_SOURCE_DIRS
            .iter()
            .map(|d| format!("\"{d}\""))
            .collect();
        entries.extend(CARRIED_FILES.iter().map(|f| format!("\"{f}\"")));
        entries.join(", ")
    }

    /// maturin's own whitelist, in the form that names a target: the wheel gets
    /// its licenses from `license-files` and its page from `readme`, so only the
    /// source distribution needs them listed as files.
    const MATURIN_INCLUDE: &str = "\
[tool.maturin]
include = [
    { path = \"CHANGELOG.md\", format = \"sdist\" },
    { path = \"LICENSE-APACHE\", format = \"sdist\" },
    { path = \"LICENSE-MIT\", format = \"sdist\" },
    { path = \"README.en.md\", format = \"sdist\" },
    { path = \"THIRD-PARTY-NOTICES.md\", format = \"sdist\" },
]
";

    fn pyproject(body: &str) -> String {
        format!(
            "[project]\nname = \"simllms\"\ndescription = \"{DESCRIPTION}\"\n\
             license = \"MIT OR Apache-2.0\"\nreadme = \"README.en.md\"\n{body}\n\
             {MATURIN_INCLUDE}"
        )
    }

    #[test]
    fn a_manifest_that_asks_for_everything_passes() {
        let manifest = crate_manifest(&carried_include(), "\"llm\", \"openai-compatible\"");
        assert_eq!(crate_problems(&manifest), Vec::<String>::new());
    }

    #[test]
    fn a_missing_include_list_is_named_as_the_root_cause() {
        let manifest = "[package]\nname = \"simllms\"\nlicense = \"MIT OR Apache-2.0\"\n";
        let problems = crate_problems(manifest);
        assert!(
            problems.iter().any(|p| p.contains("include")),
            "must say the whitelist is absent: {problems:?}"
        );
    }

    #[test]
    fn an_inherited_license_is_judged_where_it_is_declared() {
        let inherited = crate_manifest(&carried_include(), "\"openai-compatible\"").replace(
            &format!("license = \"{LICENSE}\""),
            "license.workspace = true",
        );
        assert_eq!(crate_problems(&inherited), Vec::<String>::new());

        let wrong = check_crate_manifest(&inherited, &ROOT.replace(LICENSE, "MIT"));
        assert!(
            wrong
                .iter()
                .any(|p| p.contains("[workspace.package] license")),
            "must name the root manifest as the place to fix: {wrong:?}"
        );

        let absent = ROOT.replace(&format!("license = \"{LICENSE}\"\n"), "");
        let missing = check_crate_manifest(&inherited, &absent);
        assert!(
            missing
                .iter()
                .any(|p| p.contains("no `[workspace.package] license`")),
            "{missing:?}"
        );
    }

    #[test]
    fn a_whitelist_that_forgets_the_attribution_is_red() {
        let manifest = crate_manifest(
            r#""src", "examples", "README.en.md", "LICENSE-MIT", "LICENSE-APACHE", "CHANGELOG.md""#,
            "\"llm\", \"openai-compatible\"",
        );
        let problems = crate_problems(&manifest);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("THIRD-PARTY-NOTICES.md"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_directory_entry_covers_its_contents() {
        let patterns = vec!["src".to_string(), "examples/**".to_string()];
        assert!(covers(&patterns, "src/lib.rs"));
        assert!(covers(&patterns, "examples/chat_stream.rs"));
        assert!(!covers(&patterns, "LICENSE-MIT"));
    }

    #[test]
    fn the_chinese_readme_never_renders_on_a_registry_page() {
        let manifest = crate_manifest(&carried_include(), "\"openai-compatible\"")
            .replace("README.en.md", "README.md");
        let problems = crate_problems(&manifest);
        assert!(
            problems.iter().any(|p| p.contains("English README")),
            "must say which file to point at: {problems:?}"
        );
    }

    #[test]
    fn a_description_that_names_a_provider_is_red_on_both_sides() {
        let manifest = crate_manifest(&carried_include(), "\"openai-compatible\"").replace(
            DESCRIPTION,
            "Unified client for DeepSeek, Kimi and friends.",
        );
        assert!(
            crate_problems(&manifest)
                .iter()
                .any(|p| p.contains("provider-agnostic"))
        );

        let body = "keywords = [\"llm\", \"openai-compatible\"]\nlicense-files = [\"LICENSE-APACHE\", \"LICENSE-MIT\", \"THIRD-PARTY-NOTICES.md\"]\nclassifiers = []";
        let bad = pyproject(body).replace(DESCRIPTION, "Unified client for Zhipu models");
        assert!(
            check_pyproject(&bad)
                .iter()
                .any(|p| p.contains("provider-agnostic"))
        );
    }

    #[test]
    fn the_deprecated_license_table_is_refused_by_name() {
        let body = "keywords = [\"openai-compatible\"]\nlicense-files = [\"LICENSE-APACHE\", \"LICENSE-MIT\", \"THIRD-PARTY-NOTICES.md\"]";
        let table = pyproject(body).replace(
            "license = \"MIT OR Apache-2.0\"",
            "license = { text = \"MIT OR Apache-2.0\" }",
        );
        let problems = check_pyproject(&table);
        assert!(
            problems.iter().any(|p| p.contains("deprecated")),
            "must name the PEP 639 form to use: {problems:?}"
        );
    }

    #[test]
    fn a_license_classifier_is_a_problem_next_to_an_spdx_expression() {
        let body = "keywords = [\"openai-compatible\"]\nlicense-files = [\"LICENSE-APACHE\", \"LICENSE-MIT\", \"THIRD-PARTY-NOTICES.md\"]\nclassifiers = [\n    \"License :: OSI Approved :: MIT License\",\n    \"Programming Language :: Rust\",\n]";
        let problems = check_pyproject(&pyproject(body));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("License ::"), "{problems:?}");
    }

    #[test]
    fn attribution_must_reach_an_installed_distribution() {
        let body = "keywords = [\"openai-compatible\"]\nlicense-files = [\"LICENSE-APACHE\", \"LICENSE-MIT\"]";
        let problems = check_pyproject(&pyproject(body));
        assert!(
            problems
                .iter()
                .any(|p| p.contains("THIRD-PARTY-NOTICES.md")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_license_path_that_reaches_above_the_project_is_refused() {
        let body = "keywords = [\"openai-compatible\"]\nlicense-files = [\"../../LICENSE-MIT\", \
                    \"LICENSE-APACHE\", \"THIRD-PARTY-NOTICES.md\"]";
        let problems = check_pyproject(&pyproject(body));
        assert!(
            problems.iter().any(|p| p.contains("PEP 639 forbids")),
            "the build backend would reject it: {problems:?}"
        );
    }

    #[test]
    fn crates_io_keyword_caps_are_enforced() {
        let manifest = crate_manifest(
            &carried_include(),
            "\"openai-compatible\", \"deepseek\", \"moonshot\", \"qwen\", \"zhipu\", \"glm\"",
        );
        let problems = crate_problems(&manifest);
        assert!(
            problems.iter().any(|p| p.contains("at most 5")),
            "{problems:?}"
        );

        let long = crate_manifest(&carried_include(), "\"openai-compatible-and-more\"");
        assert!(
            crate_problems(&long)
                .iter()
                .any(|p| p.contains("20 characters")),
            "a 24-character keyword must be named"
        );

        let spaced = crate_manifest(&carried_include(), "\"openai compatible\"");
        assert!(
            crate_problems(&spaced)
                .iter()
                .any(|p| p.contains("letters, digits")),
            "{spaced:?}"
        );
    }

    #[test]
    fn a_missing_cross_provider_keyword_is_named() {
        let manifest = crate_manifest(&carried_include(), "\"llm\", \"deepseek\"");
        assert!(
            crate_problems(&manifest)
                .iter()
                .any(|p| p.contains(CROSS_PROVIDER_KEYWORD))
        );
    }

    #[test]
    fn the_two_sides_must_say_the_same_things() {
        let body = "keywords = [\"llm\", \"openai-compatible\", \"deepseek\", \"moonshot\", \"qwen\", \"zhipu\", \"xiaomi\", \"openrouter\"]\nlicense-files = [\"LICENSE-APACHE\", \"LICENSE-MIT\", \"THIRD-PARTY-NOTICES.md\"]";
        let manifest = crate_manifest(&carried_include(), "\"llm\", \"openai-compatible\"");
        let pyproject = pyproject(body);
        assert_eq!(check_agreement(&manifest, &pyproject), Vec::<String>::new());

        let dropped = pyproject.replace("\"llm\", ", "");
        let problems = check_agreement(&manifest, &dropped);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("llm"), "{problems:?}");

        let split = pyproject.replace(DESCRIPTION, "Unified client for many providers");
        assert!(
            check_agreement(&manifest, &split)
                .iter()
                .any(|p| p.contains("describe the project differently"))
        );
    }

    #[test]
    fn a_copy_that_drifts_or_vanishes_is_named_with_its_fix() {
        let copies = [
            CopyRead {
                root: "LICENSE-MIT",
                dest: "crates/simllms/LICENSE-MIT",
                root_text: "body",
                copy_text: Some("body"),
            },
            CopyRead {
                root: "LICENSE-APACHE",
                dest: "crates/simllms/LICENSE-APACHE",
                root_text: "body",
                copy_text: Some("stale body"),
            },
            CopyRead {
                root: "CHANGELOG.md",
                dest: "crates/simllms-python/CHANGELOG.md",
                root_text: "body",
                copy_text: None,
            },
        ];
        let problems = copy_problems(&copies);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("does not match"), "{problems:?}");
        assert!(problems[1].contains("is missing"), "{problems:?}");
        for problem in &problems {
            assert!(
                problem.contains("sync-package"),
                "must say how to fix: {problem}"
            );
        }
    }

    #[test]
    fn every_carried_file_has_a_copy_in_both_packages() {
        let pairs = copy_pairs();
        assert_eq!(pairs.len(), CARRIED_FILES.len() * 2);
        for (file, dest) in &pairs {
            assert!(
                dest.starts_with(CRATE_DIR) || dest.starts_with(PYTHON_DIR),
                "{dest}"
            );
            assert!(dest.ends_with(file), "{dest} should mirror {file}");
        }
    }

    #[test]
    fn licensing_metadata_is_a_subset_of_what_every_package_carries() {
        for file in LICENSE_FILES {
            assert!(
                CARRIED_FILES.iter().any(|carried| carried == file),
                "`{file}` is required under `license-files`, so it has to be one of the copies \
                 each package keeps — a package cannot reference a file above its own directory"
            );
        }
    }

    #[test]
    fn the_listing_must_hold_the_licenses_and_nothing_unasked_for() {
        let manifest = crate_manifest(&carried_include(), "\"llm\", \"openai-compatible\"");
        let good: Vec<String> = CARRIED_FILES
            .iter()
            .map(|f| (*f).to_string())
            .chain(
                [
                    "src/lib.rs",
                    "examples/chat_stream.rs",
                    "Cargo.toml",
                    "Cargo.toml.orig",
                    ".cargo_vcs_info.json",
                    "Cargo.lock",
                ]
                .iter()
                .map(|f| (*f).to_string()),
            )
            .collect();
        assert_eq!(
            check_crate_listing(&good.join("\n"), &manifest),
            Vec::<String>::new()
        );

        let missing_license = good
            .iter()
            .filter(|p| *p != "LICENSE-MIT")
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let problems = check_crate_listing(&missing_license, &manifest);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("LICENSE-MIT"), "{problems:?}");

        let extra = [good.clone(), vec!["notes/scratch.md".to_string()]]
            .concat()
            .join("\n");
        let problems = check_crate_listing(&extra, &manifest);
        assert!(
            problems.iter().any(|p| p.contains("never asked for")),
            "packing more than the whitelist must go red: {problems:?}"
        );
    }

    #[test]
    fn a_windows_listing_describes_the_same_archive() {
        let manifest = crate_manifest(&carried_include(), "\"llm\", \"openai-compatible\"");
        let listing = "src\\lib.rs\nexamples\\chat_stream.rs\nREADME.en.md\n";
        let problems = check_crate_listing(listing, &manifest);
        assert!(
            !problems.iter().any(|p| p.contains("never asked for")),
            "a backslash separator is not extra content: {problems:?}"
        );
        assert!(
            !problems.iter().any(|p| p.contains("nothing under `src/`")),
            "{problems:?}"
        );
    }

    #[test]
    fn both_license_fields_at_once_is_a_cargo_error() {
        let manifest = crate_manifest(&carried_include(), "\"openai-compatible\"").replace(
            "readme = \"README.en.md\"",
            "license-file = \"LICENSE-MIT\"",
        );
        let problems = crate_problems(&manifest);
        assert!(
            problems.iter().any(|p| p.contains("cargo rejects")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("no `readme`")),
            "the missing readme pointer is a separate problem: {problems:?}"
        );
    }
}
