use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

mod docs;
mod manifest;
mod package;
mod release;
mod version;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const API_URL: &str = "https://models.dev/api.json";
const UPSTREAM_REPO: &str = "https://github.com/anomalyco/models.dev";
const UPSTREAM_BRANCH: &str = "dev";

/// models.dev provider ids we consume, mapped onto our Provider variants.
/// A provider may appear under several upstream ids (global + CN registry);
/// the first id wins on (provider, model id) collisions.
const SOURCES: &[(&str, &str)] = &[
    ("deepseek", "DeepSeek"),
    ("moonshotai", "Moonshot"),
    ("moonshotai-cn", "Moonshot"),
    ("alibaba", "DashScope"),
    ("alibaba-cn", "DashScope"),
    ("zhipuai", "Zhipu"),
    ("xiaomi", "MiMo"),
    ("openrouter", "OpenRouter"),
];

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("refresh-catalog") => {
            let check = std::env::args().nth(2).as_deref() == Some("--check");
            refresh_catalog(check);
        }
        Some("sync-version") => {
            let check = std::env::args().nth(2).as_deref() == Some("--check");
            sync_version(check);
        }
        Some("sync-package") => {
            let check = std::env::args().nth(2).as_deref() == Some("--check");
            sync_package(check);
        }
        Some("check-package") => check_package(std::env::args().skip(2)),
        Some("check-docs") => check_docs(),
        Some("check-workflow") => check_workflow(),
        Some("release-tag") => release_tag(),
        Some("check-release") => check_release(std::env::args().skip(2)),
        Some("check-artifacts") => check_artifacts(std::env::args().skip(2)),
        _ => {
            eprintln!(
                "usage: cargo xtask refresh-catalog [--check] | sync-version [--check] | \
                 sync-package [--check] | check-package [--crate-listing <file|->] | check-docs | \
                 check-workflow | release-tag | check-release <tag> [--commit <sha>] | \
                 check-artifacts <dir> --tag <tag>"
            );
            std::process::exit(2);
        }
    }
}

/// The Authoritative Version, read from the root manifest. Deriving the tag a
/// tree would deserve and checking the tag a run was given are two commands over
/// the same number.
fn authoritative_version() -> String {
    let root_manifest = read(&path(version::ROOT_MANIFEST));
    version::authoritative_version(&root_manifest).unwrap_or_else(|problem| panic!("{problem}"))
}

/// The tag this tree would be released under, on stdout: `v` and the Authoritative
/// Version. A dry-run dispatch has no tag to read, and the number a tag would have
/// to carry is the one the workspace already declares — so the derivation stays
/// here rather than becoming a shell `grep` of `Cargo.toml`.
fn release_tag() {
    println!("v{}", authoritative_version());
}

/// Is this tag allowed to become a release? Every rule is named by the check that
/// refused, so a rejected tag says which decision it failed rather than only that
/// a job went red.
fn check_release(args: impl Iterator<Item = String>) {
    let args: Vec<String> = args.collect();
    let Some(tag) = args.first().map(String::as_str) else {
        eprintln!("usage: cargo xtask check-release <vX.Y.Z> [--commit <sha>]");
        std::process::exit(2);
    };
    let commit = arg_after(&args, "--commit").unwrap_or_else(|| "HEAD".to_string());
    let authoritative = authoritative_version();
    let changelog = read(&path(release::CHANGELOG));

    let mut problems = release::check_tag(tag);
    problems.extend(release::check_tag_version(tag, &authoritative));
    problems.extend(release::check_ancestor(tag, is_ancestor_of_main(&commit)));
    if let Some(number) = release::tag_version(tag) {
        problems.extend(release::check_changelog(&changelog, number));
    }
    finish(
        problems,
        &format!("`{tag}` may be released: the version is final, current, reviewed and dated"),
    );
}

/// Is `commit` reachable from `main`? `git` answers; an unusable `main` is
/// reported as itself, because "not an ancestor" and "I could not tell" are
/// different facts and only one of them is a decision about the tag.
fn is_ancestor_of_main(commit: &str) -> bool {
    const MAIN: &str = "origin/main";
    let output = std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, MAIN])
        .status()
        .expect("run `git merge-base`");
    if output.success() {
        return true;
    }
    if output.code() == Some(1) {
        return false;
    }
    eprintln!(
        "`git merge-base --is-ancestor {commit} {MAIN}` could not decide — the release line fetches `main` before it asks (checkout with `fetch-depth: 0` and `git fetch origin {MAIN}:…`)."
    );
    std::process::exit(1);
}

/// The artifact set, judged against the intended platform set. A directory the
/// release run filled, and the tag it was asked to publish.
fn check_artifacts(args: impl Iterator<Item = String>) {
    let args: Vec<String> = args.collect();
    let Some(directory) = args.first().map(String::as_str) else {
        eprintln!("usage: cargo xtask check-artifacts <dir> --tag <vX.Y.Z>");
        std::process::exit(2);
    };
    let Some(tag) = arg_after(&args, "--tag") else {
        eprintln!(
            "`check-artifacts` needs `--tag`, or the version the artifacts have to carry is a guess."
        );
        std::process::exit(2);
    };
    let Some(version) = release::tag_version(&tag) else {
        eprintln!(
            "`{tag}` is not a release tag, so nothing here can be compared to it — pass `--tag` in `vX.Y.Z`, the number `cargo xtask release-tag` derives."
        );
        std::process::exit(1);
    };
    let entries = fs::read_dir(path(directory)).unwrap_or_else(|e| {
        panic!(
            "read the artifact directory {directory}: {e} — an empty set is a failed build, not a release"
        )
    });
    let mut names: Vec<String> = entries
        .map(|entry| {
            let entry = entry.expect("read an artifact directory entry");
            entry.file_name().to_string_lossy().into_owned()
        })
        .collect();
    names.sort();
    finish(
        release::check_artifacts(&names, version),
        &format!(
            "the artifacts are the {version} release line intended: no platform missing, none extra"
        ),
    );
}

/// The release line's own configuration, read out of the file: what it may be
/// triggered by, what it builds, who approves publication, and whether the
/// pull-request gates run again here.
fn check_workflow() {
    let ci = read(&path(release::CI_WORKFLOW));
    let problems = match read_copy(&path(release::RELEASE_WORKFLOW)) {
        None => vec![format!(
            "`{}` does not exist — the release line is a file, not an intention.",
            release::RELEASE_WORKFLOW
        )],
        Some(text) => {
            let mut problems = release::check_triggers(&text);
            problems.extend(release::check_dispatch(&text));
            problems.extend(release::check_matrix(&text));
            problems.extend(release::check_separation(&text));
            problems.extend(release::check_gate_parity(&ci, &text));
            problems.extend(release::check_invoked_gates(&text));
            problems.extend(release::check_seam_environment(&text));
            problems
        }
    };
    finish(
        problems,
        "the release line is triggered, built, gated and inventoried as the ticket says",
    );
}

/// `--flag value` in a run's arguments, if it is there.
fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

/// The guide's Rust-only list must equal the gap between what the core Client
/// offers and what the binding layer reaches.
fn check_docs() {
    let core = read(&path(docs::CORE_CLIENT));
    let bindings = read(&path(docs::BINDINGS));
    let guide = read(&path(docs::GUIDE));

    finish(
        docs::check(&core, &bindings, &guide),
        "the guide's Rust-only list matches the code",
    );
}

/// The files a published package has to carry live at the repository root, so
/// each package keeps a derived copy of them. A write run rewrites the copies;
/// `--check` only judges them, the same split `sync-version` uses.
fn sync_package(check: bool) {
    if !check {
        for (root, dest) in package::copy_pairs() {
            let source = read(&path(root));
            if read_copy(&path(&dest)).as_deref() != Some(source.as_str()) {
                fs::write(path(&dest), source).unwrap_or_else(|e| panic!("write {dest}: {e}"));
                println!("{dest} rewritten from the repository-root {root}");
            }
        }
    }
    finish(copy_problems(), "the packages' copies match the root files");
}

/// Both manifests and both sets of copies, plus the contents of a packed crate
/// when one is offered: `cargo package --list | cargo xtask check-package`.
fn check_package(args: impl Iterator<Item = String>) {
    let args: Vec<String> = args.collect();
    let mut problems = copy_problems();
    problems.extend(manifest_problems());
    problems.extend(link_problems());
    if let Some(index) = args.iter().position(|arg| arg == "--crate-listing") {
        let listing = read_listing(args.get(index + 1).map(String::as_str));
        let manifest = read(&path(package::CRATE_MANIFEST));
        problems.extend(package::check_crate_listing(&listing, &manifest));
    }
    finish(
        problems,
        "packages carry their licenses, attribution and README",
    );
}

/// The links inside the root files that every package copies. A registry page
/// renders those copies alone, so a link that only works from the repository
/// root is broken for the people who never clone it.
fn link_problems() -> Vec<String> {
    package::CARRIED_FILES
        .iter()
        .flat_map(|file| package::check_carried_links(&read(&path(file)), file))
        .collect()
}

/// The derived copies, judged against the files at the repository root.
fn copy_problems() -> Vec<String> {
    let copies: Vec<(String, String, String, Option<String>)> = package::copy_pairs()
        .into_iter()
        .map(|(root, dest)| {
            let source = read(&path(root));
            let copy = read_copy(&path(&dest));
            (root.to_string(), dest, source, copy)
        })
        .collect();
    let reads: Vec<package::CopyRead<'_>> = copies
        .iter()
        .map(|(root, dest, source, copy)| package::CopyRead {
            root,
            dest,
            root_text: source,
            copy_text: copy.as_deref(),
        })
        .collect();
    package::copy_problems(&reads)
}

/// What the two manifests declare about themselves, and where they disagree.
fn manifest_problems() -> Vec<String> {
    let root_manifest = read(&path(version::ROOT_MANIFEST));
    let crate_manifest = read(&path(package::CRATE_MANIFEST));
    let pyproject = read(&path(package::PYPROJECT));
    let mut problems = package::check_crate_manifest(&crate_manifest, &root_manifest);
    problems.extend(package::check_pyproject(&pyproject));
    problems.extend(package::check_agreement(&crate_manifest, &pyproject));
    problems
}

/// The Authoritative Version lives in the root workspace manifest; the binding
/// crate is its only written copy. Nothing here reads a tag or rewrites the
/// repository's own declaration — a human changes the source and the diff shows it.
fn sync_version(check: bool) {
    let binding_path = path(package::BINDING_MANIFEST);
    let root_manifest = read(&path(version::ROOT_MANIFEST));
    let binding_manifest = read(&binding_path);
    let pyproject = read(&path(package::PYPROJECT));
    let python_init = read(&path(package::PYTHON_INIT));

    // A version we cannot read must not be propagated; `inspect` below is what
    // names why, so dropping it here defers the report rather than swallowing it.
    let derived = version::authoritative_version(&root_manifest)
        .ok()
        .and_then(|v| version::render_derived(&binding_manifest, &v).ok());

    // `--check` judges what is on disk, so a stale derived copy is named rather
    // than quietly repaired; only a write run replaces it.
    let effective = match derived {
        Some(rendered) if !check && rendered != binding_manifest => {
            fs::write(&binding_path, &rendered).expect("write the binding crate's manifest");
            println!(
                "derived {} from the Authoritative Version",
                package::BINDING_MANIFEST
            );
            rendered
        }
        _ => binding_manifest,
    };

    let problems = version::inspect(&root_manifest, &effective, &pyproject, &python_init);
    finish(problems, "versions are consistent");
}

/// Name every problem and fail, or say the one line that nothing is wrong.
fn finish(problems: Vec<String>, success: &str) {
    if !problems.is_empty() {
        for problem in &problems {
            eprintln!("{problem}");
        }
        std::process::exit(1);
    }
    println!("{success}");
}

/// `--crate-listing -` takes `cargo package --list` output from a pipe, so the
/// contents of an archive get checked without anyone reading a `tar` listing.
fn read_listing(source: Option<&str>) -> String {
    match source.unwrap_or("-") {
        "-" => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .expect("read the crate listing from stdin");
            buf
        }
        file => read(&PathBuf::from(file)),
    }
}

fn path(relative: &str) -> PathBuf {
    PathBuf::from(ROOT).join(relative)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "read {} (run from the repository root): {e}",
            path.display()
        )
    })
}

/// A copy that is not there yet is a finding, not a crash; any other failure to
/// read it is a broken checkout and says so.
fn read_copy(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => panic!("read {}: {e}", path.display()),
    }
}

fn refresh_catalog(check: bool) {
    let snapshot_path = path("catalog/modelsdev-snapshot.json");
    let data_path = path("crates/simllms/src/catalog_data.rs");

    if check {
        // Offline consistency: the generated code must match the vendored
        // snapshot. Staleness against upstream is the weekly bump's job —
        // the PR gate must never go red because models.dev moved.
        let snapshot: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&snapshot_path).expect("vendored snapshot exists"),
        )
        .expect("vendored snapshot parses");
        let code = generate_catalog_data(&snapshot);
        if fs::read_to_string(&data_path).unwrap_or_default() != code {
            eprintln!(
                "crates/simllms/src/catalog_data.rs does not match the vendored snapshot; run `cargo xtask refresh-catalog`"
            );
            std::process::exit(1);
        }
        println!("catalog is up to date");
        return;
    }

    let snapshot = fetch_snapshot();
    let snapshot_json = serde_json::to_string_pretty(&snapshot).expect("snapshot serializes");
    let code = generate_catalog_data(&snapshot);

    if let Some(dir) = snapshot_path.parent() {
        fs::create_dir_all(dir).expect("create catalog dir");
    }
    fs::write(&snapshot_path, snapshot_json).expect("write snapshot");
    fs::write(&data_path, code).expect("write generated catalog data");
    println!(
        "refreshed {} and {}",
        snapshot_path.display(),
        data_path.display()
    );
}

fn fetch_snapshot() -> serde_json::Value {
    let raw = get(API_URL).expect("fetch models.dev api.json");
    let all: serde_json::Value = serde_json::from_str(&raw).expect("api.json parses");
    let mut providers = BTreeMap::new();
    for (source_id, _) in SOURCES {
        if let Some(p) = all.get(*source_id) {
            providers.insert((*source_id).to_string(), p.clone());
        } else {
            eprintln!("warning: upstream has no provider `{source_id}`");
        }
    }

    let (commit, date) = fetch_upstream_commit();
    serde_json::json!({
        "meta": {
            "source": UPSTREAM_REPO,
            "upstream_url": API_URL,
            "commit": commit,
            "date": date,
            "license": "MIT",
        },
        "providers": providers,
    })
}

fn fetch_upstream_commit() -> (String, String) {
    let raw = get(&format!(
        "https://api.github.com/repos/anomalyco/models.dev/commits/{UPSTREAM_BRANCH}"
    ))
    .expect("fetch upstream commit");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("commit json parses");
    let sha = v
        .get("sha")
        .and_then(|s| s.as_str())
        .expect("commit has sha")
        .to_string();
    let date = v
        .pointer("/commit/committer/date")
        .and_then(|d| d.as_str())
        .expect("commit has date")
        .to_string();
    (sha, date)
}

fn get(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let body = ureq::get(url)
        .set("User-Agent", "simllms-xtask")
        .timeout(std::time::Duration::from_secs(60))
        .call()?
        .into_string()?;
    Ok(body)
}

struct Entry {
    input: Vec<&'static str>,
    structured_output: bool,
    tool_call: bool,
    reasoning: String,
    context: Option<u64>,
    output: Option<u64>,
    status: &'static str,
}

fn generate_catalog_data(snapshot: &serde_json::Value) -> String {
    let mut out = String::new();
    let (commit, date) = (
        snapshot
            .pointer("/meta/commit")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        snapshot
            .pointer("/meta/date")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
    );
    out.push_str("// GENERATED by `cargo xtask refresh-catalog` from the vendored models.dev\n");
    out.push_str(&format!(
        "// snapshot ({commit}, {date}, MIT). DO NOT EDIT BY HAND.\n"
    ));
    out.push_str("// Rerun `cargo xtask refresh-catalog` after changing the snapshot.\n");
    out.push_str(
        "use crate::capabilities::{Capabilities, Modality, ModelStatus, ReasoningMode, Source};\n",
    );
    out.push_str("use crate::provider::Provider;\n");
    out.push_str("use std::collections::BTreeSet;\n\n");
    out.push_str("pub(crate) const SNAPSHOT_COMMIT: &str = \"");
    out.push_str(commit);
    out.push_str("\";\n\n");

    out.push_str("#[rustfmt::skip]\nfn mk(\n    input: &[Modality],\n    structured_output: bool,\n    tool_call: bool,\n    reasoning: ReasoningMode,\n    context: Option<u64>,\n    output: Option<u64>,\n    status: ModelStatus,\n) -> Capabilities {\n    Capabilities {\n        source: Source::Catalog,\n        input_modalities: input.iter().copied().collect(),\n        structured_output,\n        tool_call,\n        image_generation: false,\n        reasoning,\n        context_window: context,\n        max_output_tokens: output,\n        status,\n    }\n}\n\n");
    out.push_str(
        "#[rustfmt::skip]\npub(crate) fn entries() -> Vec<(Provider, &'static str, Capabilities)> {\n    vec![\n",
    );

    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut count = 0usize;
    for (source_id, variant) in SOURCES {
        let models = snapshot
            .pointer(&format!("/providers/{source_id}/models"))
            .and_then(|m| m.as_object());
        let Some(models) = models else { continue };
        let mut ids: Vec<&String> = models.keys().collect();
        ids.sort();
        for id in ids {
            if !seen.insert((variant.to_string(), id.clone())) {
                continue;
            }
            let m = &models[id.as_str()];
            let entry = map_model(m);
            out.push_str(&format!(
                "        (Provider::{variant}, {:?}, mk(\n            &[{}],\n            {},\n            {},\n            {},\n            {},\n            {},\n            ModelStatus::{},\n        )),\n",
                id,
                entry.input.iter().map(|m| format!("Modality::{m}")).collect::<Vec<_>>().join(", "),
                entry.structured_output,
                entry.tool_call,
                entry.reasoning,
                opt_u64(entry.context),
                opt_u64(entry.output),
                entry.status,
            ));
            count += 1;
        }
    }

    out.push_str("    ]\n}\n");
    eprintln!("generated {count} entries");
    out
}

fn map_model(m: &serde_json::Value) -> Entry {
    let input: Vec<&'static str> = m
        .pointer("/modalities/input")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| match *s {
                    "text" => true,
                    "image" => true,
                    _ => false, // video/pdf/audio are out of scope for v1
                })
                .map(|s| match s {
                    "image" => "Image",
                    _ => "Text",
                })
                .collect()
        })
        .unwrap_or_default();
    let mut input = input;
    if input.is_empty() {
        input.push("Text");
    }

    let options = m.get("reasoning_options").and_then(|v| v.as_array());
    let reasoning = match options {
        Some(opts) => {
            let levels: BTreeSet<String> = opts
                .iter()
                .filter_map(|o| o.get("values"))
                .filter_map(|v| v.as_array())
                .flat_map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)))
                .collect();
            if !levels.is_empty() {
                // Effort subsumes toggle: choosing a level is the on/off.
                format!(
                    "ReasoningMode::Effort {{ levels: BTreeSet::from([{}]) }}",
                    levels
                        .iter()
                        .map(|l| format!("{l:?}.into()"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                "ReasoningMode::Toggle".into()
            }
        }
        None => {
            if m.get("reasoning").and_then(|v| v.as_bool()) == Some(true) {
                "ReasoningMode::Toggle".into()
            } else {
                "ReasoningMode::None".into()
            }
        }
    };

    Entry {
        input,
        structured_output: m.get("structured_output").and_then(|v| v.as_bool()) == Some(true),
        tool_call: m.get("tool_call").and_then(|v| v.as_bool()) == Some(true),
        reasoning,
        context: m.pointer("/limit/context").and_then(|v| v.as_u64()),
        output: m.pointer("/limit/output").and_then(|v| v.as_u64()),
        status: if m.get("status").and_then(|v| v.as_str()) == Some("deprecated") {
            "Deprecated"
        } else {
            "Active"
        },
    }
}

fn opt_u64(v: Option<u64>) -> String {
    match v {
        Some(n) => format!("Some({n})"),
        None => "None".into(),
    }
}
