use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

mod docs;
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
        Some("check-docs") => check_docs(),
        _ => {
            eprintln!(
                "usage: cargo xtask refresh-catalog [--check] | sync-version [--check] | check-docs"
            );
            std::process::exit(2);
        }
    }
}

/// The guide's Rust-only list must equal the gap between what the core Client
/// offers and what the binding layer reaches.
fn check_docs() {
    let core = read(&path(docs::CORE_CLIENT));
    let bindings = read(&path(docs::BINDINGS));
    let guide = read(&path(docs::GUIDE));

    let problems = docs::check(&core, &bindings, &guide);
    if problems.is_empty() {
        println!("the guide's Rust-only list matches the code");
        return;
    }
    for problem in &problems {
        eprintln!("{problem}");
    }
    std::process::exit(1);
}

/// The Authoritative Version lives in the root workspace manifest; the binding
/// crate is its only written copy. Nothing here reads a tag or rewrites the
/// repository's own declaration — a human changes the source and the diff shows it.
fn sync_version(check: bool) {
    let binding_path = path(version::BINDING_MANIFEST);
    let root_manifest = read(&path(version::ROOT_MANIFEST));
    let binding_manifest = read(&binding_path);
    let pyproject = read(&path(version::PYPROJECT));
    let python_init = read(&path(version::PYTHON_INIT));

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
                version::BINDING_MANIFEST
            );
            rendered
        }
        _ => binding_manifest,
    };

    let problems = version::inspect(&root_manifest, &effective, &pyproject, &python_init);
    if problems.is_empty() {
        println!("versions are consistent");
        return;
    }
    for problem in &problems {
        eprintln!("{problem}");
    }
    std::process::exit(1);
}

fn path(relative: &str) -> PathBuf {
    PathBuf::from(ROOT).join(relative)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {} (run from the repository root): {e}", path.display()))
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
