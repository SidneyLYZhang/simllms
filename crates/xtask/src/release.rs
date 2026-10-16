//! The release line's rules, judged from text and file names — no network, no
//! registry, no `git`. Whether a tag may be published is decided here, from the
//! declarations in the repository, so the verdict a person reads in CI is one
//! they can reproduce on a laptop.
//!
//! The workflow checks read YAML the way [`crate::manifest`] reads TOML:
//! deliberately shallow. A release workflow structured so cleverly that this scan
//! misreads it is the mistake the check exists to catch, and a real YAML parser
//! would let it through while quietly accepting the misplacement.

/// The workflow that gates a pull request, and the one that releases. Parity
/// between them is the point: a check that lives only in the release path is
/// never exercised by the work that made it necessary.
pub const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
pub const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";
pub const CHANGELOG: &str = "CHANGELOG.md";

/// The manylinux baseline the wheel matrix is built inside, named once so the pin
/// in the workflow and the tag on the artifact are checked against one number. A
/// pinned base image that nothing verifies is a pin that drifts.
pub const MANYLINUX: &str = "manylinux_2_28";

/// The glibc number [`MANYLINUX`] promises, spelled the way a wheel's platform
/// tag spells it. Held equal to `MANYLINUX` by a test rather than by a comment,
/// because the two literals sit apart and only a check notices when one moves.
pub const GLIBC_BASELINE: &str = "2_28";

/// The platforms a release owes exactly one wheel to. `abi3` is why this is a
/// platform list and not a platform-by-interpreter list: the one wheel in each
/// row covers every interpreter the package supports.
pub const WHEEL_PLATFORMS: &[(&str, &str, &str)] = &[
    (
        "manylinux x86_64",
        "linux_x86_64",
        "x86_64-unknown-linux-gnu",
    ),
    (
        "manylinux aarch64",
        "linux_aarch64",
        "aarch64-unknown-linux-gnu",
    ),
    ("macOS x86_64", "macos_x86_64", "x86_64-apple-darwin"),
    ("macOS arm64", "macos_arm64", "aarch64-apple-darwin"),
    ("Windows amd64", "windows_amd64", "x86_64-pc-windows-msvc"),
];

/// The dispatch input that means *run everything, publish nothing*.
pub const DRY_RUN_INPUT: &str = "dry_run";

/// The tag's version, when the tag is a release tag at all — `v0.1.0` yields
/// `0.1.0`.
pub fn tag_version(tag: &str) -> Option<&str> {
    let numbers = tag.strip_prefix('v')?;
    let parts: Vec<&str> = numbers.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    parts
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(numbers)
}

/// Is `tag` a version tag? A pre-release or build suffix is refused: this line
/// publishes final versions to a public registry, and a `0.1.0-rc1` that reaches
/// crates.io or PyPI is a version somebody installs and cannot take back.
pub fn check_tag(tag: &str) -> Vec<String> {
    if tag_version(tag).is_some() {
        return Vec::new();
    }
    let rule = if tag.contains('-') || tag.contains('+') {
        "a pre-release or build suffix is refused — a release candidate stays unpublished"
    } else if !tag.starts_with('v') {
        "a version tag starts with `v`"
    } else {
        "a version tag is `v` and exactly three dot-separated numbers"
    };
    vec![format!("`{tag}` is not a release tag: {rule}.")]
}

/// Does the tag name the version the repository declares? A tag ahead of the
/// Authoritative Version releases binaries that say something other than the
/// number they were released under; one behind it releases a stale tree.
pub fn check_tag_version(tag: &str, authoritative: &str) -> Vec<String> {
    let Some(version) = tag_version(tag) else {
        // `check_tag` has already named the shape; there is nothing to compare.
        return Vec::new();
    };
    if version == authoritative {
        return Vec::new();
    }
    vec![format!(
        "`{tag}` asks to publish `{version}`, but the Authoritative Version is `{authoritative}` — change the root workspace manifest and let the copies derive, or tag the number the tree already holds."
    )]
}

/// Is the tagged commit reachable from `main`? The caller asks `git`; the rule is
/// named here. A tag on a side branch would publish a build nobody reviewed and
/// that no later `main` can reproduce.
pub fn check_ancestor(tag: &str, ancestor_of_main: bool) -> Vec<String> {
    if ancestor_of_main {
        return Vec::new();
    }
    vec![format!(
        "`{tag}` points at a commit that is not an ancestor of `main` — tag a commit that has landed, so the published artifact is the reviewed tree."
    )]
}

/// The changelog has to say what this version is and when it was released. A
/// release without a dated section hands out an artifact that no history
/// explains.
pub fn check_changelog(text: &str, version: &str) -> Vec<String> {
    let header = format!("## [{version}]");
    let Some(line) = text
        .lines()
        .map(str::trim_end)
        .find(|line| line.starts_with(&header))
    else {
        return vec![format!(
            "`{CHANGELOG}` has no `## [{version}]` section — a release is recorded there before it is tagged."
        )];
    };
    let rest = line[header.len()..].trim_start();
    let Some(date) = rest.strip_prefix("- ").map(str::trim) else {
        return vec![format!(
            "`{CHANGELOG}` records `{line}` without a date — the header is `## [{version}] - YYYY-MM-DD`."
        )];
    };
    if is_dated(date) {
        return Vec::new();
    }
    vec![format!(
        "`{CHANGELOG}` dates `[{version}]` as `{date}` — the header is `## [{version}] - YYYY-MM-DD`."
    )]
}

/// `YYYY-MM-DD`, and nothing that leaves a placeholder inside a published
/// history.
fn is_dated(date: &str) -> bool {
    let parts: Vec<&str> = date.split('-').collect();
    parts.len() == 3
        && parts
            .iter()
            .zip([4, 2, 2])
            .all(|(part, width)| part.len() == width && part.bytes().all(|b| b.is_ascii_digit()))
}

/// The rules the pull-request gate holds, as the release line has to hold them:
/// every check `ci.yml` runs appears again in the release workflow. Read out of
/// `ci` rather than listed here, so a new gate becomes a release requirement by
/// itself instead of waiting for someone to add it in two places.
pub fn check_gate_parity(ci: &str, release: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let released = gates(release);
    for gate in gates(ci) {
        if !released.contains(&gate) {
            problems.push(format!(
                "the pull-request gate runs `{gate}` and `{RELEASE_WORKFLOW}` does not — a release must not rely on a check that only exists in the pull-request path."
            ));
        }
    }
    problems
}

/// The command shapes that are checks rather than builds: the test suites and
/// the gate family, on either side of the binding. A shape is a prefix, and the
/// word after it is what identifies the check — the `xtask` subcommand, the
/// suite, the tool. Build steps (`cargo package`, `uv pip install`) are
/// deliberately absent: the release line builds in its own way, through its own
/// packagers, and demanding its shape from the pull-request file would only
/// teach the check to complain.
const GATE_SHAPES: &[&str] = &["cargo test ", "cargo xtask ", "uv run --no-sync "];

/// Every gate a workflow runs, in order, deduped. Comments are stripped first: a
/// file that only *mentions* a gate in a comment does not run it. A step's own
/// title is cut for the same reason — `- name: cargo xtask check-docs` describes
/// a step and runs nothing.
fn gates(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in code(text).lines() {
        if line.starts_with("name:") || line.starts_with("- name:") {
            continue;
        }
        for shape in GATE_SHAPES {
            let mut rest = line;
            while let Some(index) = rest.find(shape) {
                rest = &rest[index + shape.len()..];
                if let Some(word) = rest.split_whitespace().next() {
                    let gate = format!("{shape}{word}");
                    if !found.contains(&gate) {
                        found.push(gate);
                    }
                }
            }
        }
    }
    found
}

/// The verdicts that decide whether a tag may be released, and the inputs the
/// artifact seam is nothing without. They are named here because a rule that
/// exists only as a function is a rule that never fires: `check-workflow` reads
/// the workflow's shape, and a deleted step leaves a shape that is still valid.
/// So the release line has to hold its own gates inside itself.
pub fn check_invoked_gates(text: &str) -> Vec<String> {
    const INVOKED: &[&str] = &["check-release", "check-artifacts"];
    let mut problems = Vec::new();
    let run = code(text);
    for gate in INVOKED {
        if !run.contains(&format!("cargo xtask {gate}")) {
            problems.push(format!(
                "`{RELEASE_WORKFLOW}` never runs `cargo xtask {gate}` — a rule nobody invokes cannot stop a Release, and the tag or the artifact set goes out ungated."
            ));
        }
    }
    // Assigned, not merely mentioned: a step that is only *called* by these names
    // hands the seam nothing.
    let assigned = |name: &str| {
        run.lines()
            .any(|line| line.contains(&format!("{name}:")) || line.contains(&format!("{name}=")))
    };
    if !assigned("SIMLLMS_RELEASE_TAG") {
        problems.push(
            format!("`{RELEASE_WORKFLOW}` never assigns `SIMLLMS_RELEASE_TAG`, so the seam suite has no tag to compare the artifacts against — every release assertion in it skips, and the run reports green having tested nothing."),
        );
    }
    let installs_a_named_artifact = run
        .lines()
        .any(|line| line.contains("pip install") && line.contains("SIMLLMS_WHEEL"));
    if !installs_a_named_artifact {
        problems.push(format!(
            "`{RELEASE_WORKFLOW}` installs no `$SIMLLMS_WHEEL`, so the seam suite imports this checkout rather than the artifact a Registry would hand out — name the wheel it downloads and install that."
        ));
    }
    problems
}

/// What the workflow is allowed to be triggered by: a version tag, and a hand
/// dispatch that can declare itself a rehearsal. Everything else — a comment on
/// an issue, a pull request, a schedule — is a way to run the release line by
/// accident, from a message that was never meant as one.
pub fn check_triggers(text: &str) -> Vec<String> {
    const ALLOWED: &[&str] = &["push", "workflow_dispatch"];
    let mut problems = Vec::new();
    let triggers = mapping_keys(text, &["on"]);
    for trigger in &triggers {
        if !ALLOWED.contains(&trigger.as_str()) {
            problems.push(format!(
                "`{RELEASE_WORKFLOW}` triggers on `{trigger}` — only a version tag (`push` with a tag filter) and a hand dispatch may start a release."
            ));
        }
    }
    for trigger in ALLOWED {
        if !triggers.iter().any(|found| found == *trigger) {
            problems.push(format!(
                "`{RELEASE_WORKFLOW}` does not trigger on `{trigger}` — the release line is exercised by a tag and by a dry-run dispatch, both."
            ));
        }
    }
    if triggers.iter().any(|t| t == "push") {
        let patterns = scalar(text, &["on", "push"], "tags");
        if !patterns.iter().any(|pattern| pattern.starts_with('v')) {
            problems.push(format!(
                "`{RELEASE_WORKFLOW}`'s `push` trigger carries no `v*` tag filter, so a push of any branch reaches the release checks."
            ));
        }
    }
    problems
}

/// A hand dispatch has to be able to say *rehearse*: the only difference between
/// a dry run and a release is what the publish jobs may touch, and a line that
/// cannot rehearse is a release that finds its bugs in front of the registries.
/// The tag itself is never an input — a rehearsal takes its version from the
/// Authoritative Version of the commit it is dispatched on, which is the number a
/// tag would have to carry.
pub fn check_dispatch(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let inputs = mapping_keys(text, &["on", "workflow_dispatch", "inputs"]);
    if inputs.is_empty() {
        return vec![format!(
            "`{RELEASE_WORKFLOW}`'s `workflow_dispatch` declares no `inputs`, so the release line cannot be run without publishing."
        )];
    }
    if !inputs.iter().any(|input| input == DRY_RUN_INPUT) {
        problems.push(format!(
            "the dispatch inputs have no `{DRY_RUN_INPUT}`, which is how a rehearsal is asked for."
        ));
    }
    if inputs.iter().any(|input| input == "tag") {
        problems.push(
            "the dispatch inputs take a `tag` — a rehearsal should name itself and read its version from the Authoritative Version, or a dry run proves a different code path from a release.".to_string(),
        );
    }
    problems
}

/// The wheel matrix, as the ticket asks for it: every intended platform built, the
/// base image named rather than defaulted, the arm build crossed rather than
/// emulated, and one abi3 wheel per platform.
pub fn check_matrix(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let body = code(text);

    if !body.contains(MANYLINUX) {
        problems.push(format!(
            "`{RELEASE_WORKFLOW}` never names `{MANYLINUX}` — the manylinux baseline has to be pinned, not left to whatever the builder defaults to today."
        ));
    }
    for forbidden in ["qemu", "binfmt"] {
        if body.to_lowercase().contains(forbidden) {
            problems.push(format!(
                "`{RELEASE_WORKFLOW}` runs on `{forbidden}` — the aarch64 wheel is cross-compiled in a cross image; an emulator is slower and is a different toolchain from the one the wheel was tested with."
            ));
        }
    }
    for (label, _, target) in WHEEL_PLATFORMS {
        if !body.contains(target) {
            problems.push(format!(
                "the wheel matrix has no `{target}` row, so {label} is not built — every platform in the intended set is built or the release is not complete."
            ));
        }
    }
    let images = values_of_key(text, "container");
    let crossed = WHEEL_PLATFORMS
        .iter()
        .filter(|(label, _, _)| label.starts_with("manylinux"))
        .count();
    if images.len() < crossed {
        problems.push(format!(
            "`{RELEASE_WORKFLOW}` names {crossed} manylinux platforms but only {} base image{}, so a row builds inside whatever image the builder defaults to today — name the image for every manylinux row, or drop the row.",
            images.len(),
            if images.len() == 1 { "" } else { "s" }
        ));
    }
    for image in images {
        let Some(tag) = image.rsplit(':').next().filter(|tag| !tag.is_empty()) else {
            problems.push(format!(
                "the container `{image}` carries no tag — pin it to a dated release, as `{MANYLINUX}`'s own image is pinned, or the base moves under a build and takes the artifact with it."
            ));
            continue;
        };
        if tag == "latest" || !image.contains('/') {
            problems.push(format!(
                "the container `{image}` is not pinned — a base image that moves under the build moves the artifact, and the glibc baseline the release promises is then whatever the image decided today."
            ));
        }
        if !image.contains(MANYLINUX) {
            problems.push(format!(
                "the container `{image}` names no `{MANYLINUX}` baseline, while `{MANYLINUX}` is what the wheel names are inspected against — a wheel built against one baseline and read as another is the mistake the pin exists to stop."
            ));
        }
    }
    if !body.contains("abi3") {
        problems.push(format!(
            "`{RELEASE_WORKFLOW}` never names `abi3` — one wheel per platform is what the binding's `abi3-py310` feature promises, and a per-interpreter matrix would silently narrow the support range."
        ));
    }
    problems
}

/// Build and publish are separate jobs joined by artifacts, so the approval gate
/// holds *publication* and not computation: a build needs no approval and can run
/// as often as it likes, while the one step that reaches a registry stands behind
/// a reviewer.
pub fn check_separation(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let jobs = mapping_keys(text, &["jobs"]);
    if jobs.is_empty() {
        return vec![format!(
            "`{RELEASE_WORKFLOW}` declares no jobs — the release line is a `jobs:` mapping with a build job and a publishing job in it."
        )];
    }
    let mut publishers: Vec<&String> = Vec::new();
    for job in &jobs {
        let body = job_body(text, job);
        // A dry run of the packager publishes nothing, so it is not a publisher;
        // it is the rehearsal of one.
        let reaches_registry = lines_of(&body).iter().any(|line| {
            (line.contains("gh-action-pypi-publish")
                || line.contains("twine upload")
                || (line.contains("cargo publish") && !line.contains("--dry-run")))
                && !line.trim_start().starts_with('#')
        });
        if reaches_registry {
            publishers.push(job);
        }
    }
    if publishers.is_empty() {
        problems.push(format!(
            "no job in `{RELEASE_WORKFLOW}` reaches a registry, so this is not a release line — the crate is published and the wheel and sdist are uploaded."
        ));
    }
    for job in publishers {
        let path = ["jobs", job.as_str()];
        if scalar(text, &path, "environment").is_empty() {
            problems.push(format!(
                "the publishing job `{job}` is bound to no GitHub environment — the approval gate lives on the environment, and without one the workflow publishes on its own authority."
            ));
        }
        if scalar(text, &path, "needs").is_empty() {
            problems.push(format!(
                "the publishing job `{job}` needs nothing, so it computes rather than consumes: publication has to be its own job that takes the built artifacts as input."
            ));
        }
        let conditions = scalar(text, &path, "if");
        if !conditions.iter().any(|line| line.contains(DRY_RUN_INPUT)) {
            problems.push(format!(
                "the publishing job `{job}` runs whatever `{DRY_RUN_INPUT}` says — a rehearsal skips it *visibly*, because a job that is simply absent from a run proves nothing."
            ));
        }
    }
    problems
}

/// The artifact names a run produced, judged against the intended set. Both
/// directions fail: a missing platform is an incomplete release, and an extra is
/// a wheel nobody agreed to release — one that cannot be taken back once a
/// Registry has it.
pub fn check_artifacts(names: &[String], version: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut wheels: Vec<(String, String)> = Vec::new();
    let mut sdists = 0usize;
    let mut crates = 0usize;

    for name in names {
        if let Some(stem) = name.strip_suffix(".whl") {
            let Some((wheel_version, abi, platform)) = wheel_fields(stem) else {
                problems.push(format!(
                    "`{name}` is not a five-field wheel name, so nothing can be said about what it carries."
                ));
                continue;
            };
            if wheel_version != version {
                problems.push(format!(
                    "`{name}` reports `{wheel_version}` while the tag is `{version}` — the version originates from the binary, so a wheel built from another tree is not this release."
                ));
            }
            if abi != "abi3" {
                problems.push(format!(
                    "`{name}` carries the ABI `{abi}`, a wheel built for one interpreter instead of the `abi3` wheel that covers every supported Python."
                ));
            }
            if let Some(baseline) = glibc_baseline(&platform) {
                if baseline != GLIBC_BASELINE {
                    problems.push(format!(
                        "`{name}` is `{platform}`, a glibc baseline of {baseline} while the matrix pins `{MANYLINUX}`."
                    ));
                }
            } else if platform.starts_with("linux_") {
                problems.push(format!(
                    "`{name}` is a bare `{platform}` wheel: not manylinux, so it claims a glibc newer than any baseline the release agreed to."
                ));
            }
            wheels.push((identity(&platform), name.clone()));
        } else if let Some(rest) = name.strip_suffix(".tar.gz") {
            match rest.strip_prefix("simllms-") {
                Some(v) if v == version => sdists += 1,
                Some(v) => {
                    problems.push(format!("`{name}` is sdist version `{v}`, not `{version}`."))
                }
                None => problems.push(format!("`{name}` is a source archive of something else.")),
            }
        } else if let Some(rest) = name.strip_suffix(".crate") {
            match rest.strip_prefix("simllms-") {
                Some(v) if v == version => crates += 1,
                Some(v) => {
                    problems.push(format!("`{name}` is crate version `{v}`, not `{version}`."))
                }
                None => problems.push(format!("`{name}` is a crate of something else.")),
            }
        } else {
            problems.push(format!(
                "`{name}` is in the artifact set but is no wheel, sdist or crate — an unexplained file is not evidence."
            ));
        }
    }

    for (label, expected, _) in WHEEL_PLATFORMS {
        let built: Vec<&String> = wheels
            .iter()
            .filter(|(identity, _)| identity == expected)
            .map(|(_, name)| name)
            .collect();
        match built.len() {
            0 => problems.push(format!(
                "no wheel for {label} (`{expected}`): the intended platform set is not complete."
            )),
            1 => {}
            count => problems.push(format!(
                "{count} wheels for {label} (`{expected}`): {:?} — one abi3 wheel per platform is the promise, and a second is a wheel nobody intended.",
                built.iter().map(|name| name.as_str()).collect::<Vec<_>>()
            )),
        }
    }
    for (identity, name) in &wheels {
        if !WHEEL_PLATFORMS
            .iter()
            .any(|(_, expected, _)| expected == identity)
        {
            problems.push(format!(
                "`{name}` is a wheel for `{identity}`, outside the intended platform set — publishing it is a decision the ticket did not make."
            ));
        }
    }
    if sdists != 1 {
        problems.push(format!(
            "the artifact set holds {sdists} sdists, expected exactly one — the sdist is how a platform outside the matrix installs the package."
        ));
    }
    if crates != 1 {
        problems.push(format!(
            "the artifact set holds {crates} crates, expected exactly one — the crate is the cargo half of the release."
        ));
    }
    problems
}

/// A wheel name's version, ABI and platform tags. Neither the distribution nor
/// the build tag holds a dash in this project, so four dashes split the name.
fn wheel_fields(stem: &str) -> Option<(String, String, String)> {
    let fields: Vec<&str> = stem.split('-').collect();
    match fields.as_slice() {
        [_, version, _, abi, platform] => {
            Some((version.to_string(), abi.to_string(), platform.to_string()))
        }
        _ => None,
    }
}

/// The glibc baseline a manylinux platform tag promises: `2_28` in
/// `manylinux_2_28_aarch64`, and nothing for a wheel that is not manylinux.
fn glibc_baseline(platform: &str) -> Option<&str> {
    let (os, version, _) = platform_parts(platform)?;
    (os == "linux").then_some(version?)
}

/// The os names a platform tag can start with, and the bare `linux_` case that
/// means "not any manylinux at all".
const OS_PREFIXES: &[(&str, &str)] = &[
    ("manylinux_", "linux"),
    ("musllinux_", "musl"),
    ("macosx_", "macos"),
    ("win_", "windows"),
    ("linux_", "bare-linux"),
];

/// A platform tag reduced to the `(os, version, arch)` triple the intended set is
/// written in. macOS deployment numbers move with the SDK, so `macosx_11_0_arm64`
/// and `macosx_12_0_arm64` are the same platform; the arch may itself hold an
/// underscore, so it is whatever the two-part numeric version leaves behind.
fn platform_parts(platform: &str) -> Option<(&'static str, Option<&str>, &str)> {
    let (prefix, os) = OS_PREFIXES
        .iter()
        .find(|(prefix, _)| platform.starts_with(prefix))?;
    let tail = &platform[prefix.len()..];
    let fields: Vec<&str> = tail.split('_').collect();
    let numeric = fields.len() > 2
        && fields[..2]
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()));
    if !numeric {
        return Some((os, None, tail));
    }
    let cut = fields[0].len() + 1 + fields[1].len() + 1;
    Some((os, Some(&tail[..cut - 1]), &tail[cut..]))
}

/// The `(os, arch)` key the intended platform set is written in. A tag from an os
/// the matrix never builds keeps its own name, so it reads as the extra it is.
fn identity(platform: &str) -> String {
    match platform_parts(platform) {
        Some((os, _, arch)) => format!("{os}_{arch}"),
        None => format!("other_{platform}"),
    }
}

/// Everything a workflow actually runs or uses, with comments cut out. A rule
/// about what a release does is not satisfied by a comment that says the words.
fn code(text: &str) -> String {
    let mut out = String::new();
    for line in lines_of(text) {
        if line.starts_with('#') {
            continue;
        }
        out.push_str(line.split(" #").next().unwrap_or(line).trim());
        out.push('\n');
    }
    out
}

/// The file's lines, indentation removed, empty lines dropped.
fn lines_of(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// The value written after every `key:` anywhere in the file — the flat read
/// behind things like the container images, which sit at whatever depth the step
/// that passes them happens to be at. A templated value is not a value.
fn values_of_key(text: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}:");
    let mut values = Vec::new();
    for line in lines_of(text) {
        if line.starts_with('#') {
            continue;
        }
        let Some(value) = line.strip_prefix(prefix.as_str()).map(str::trim) else {
            continue;
        };
        if !value.is_empty() && !value.contains("{{") {
            values.push(unquote(value));
        }
    }
    values
}

/// The keys directly under the mapping at `path`: the trigger names under `on:`,
/// the job ids under `jobs:`.
fn mapping_keys(text: &str, path: &[&str]) -> Vec<String> {
    let Some(block) = block(text, path) else {
        return Vec::new();
    };
    let Some(own) = block.iter().map(|(indent, _)| *indent).min() else {
        return Vec::new();
    };
    block
        .iter()
        .filter(|(indent, _)| *indent == own)
        .filter_map(|(_, line)| line.split_once(':'))
        .map(|(key, _)| key.trim().to_string())
        .filter(|key| !key.is_empty() && !key.starts_with('-'))
        .collect()
}

/// A key's value inside the mapping at `path` — an inline scalar, the items of a
/// flow sequence, or the items of a block sequence, one entry per way it is
/// written. A flow sequence reads as its members rather than as its brackets:
/// `needs: [inventory, seam]` names two jobs, and a rule that compares a job name
/// would otherwise miss both of them.
fn scalar(text: &str, path: &[&str], key: &str) -> Vec<String> {
    let Some(block) = block(text, path) else {
        return Vec::new();
    };
    let own = block.iter().map(|(indent, _)| *indent).min();
    let mut values: Vec<String> = Vec::new();
    let mut collecting = false;
    for (indent, line) in &block {
        let named = line
            .split_once(':')
            .is_some_and(|(name, _)| name.trim() == key && Some(*indent) == own)
            && !line.starts_with('-');
        if named {
            let rest = line.split_once(':').expect("a named key").1.trim();
            collecting = rest.is_empty();
            match rest
                .strip_prefix('[')
                .and_then(|inner| inner.strip_suffix(']'))
            {
                Some(members) => values.extend(
                    members
                        .split(',')
                        .map(|member| unquote(member.trim()))
                        .filter(|member| !member.is_empty()),
                ),
                None if !rest.is_empty() => values.push(unquote(rest)),
                None => {}
            }
            continue;
        }
        if !collecting {
            continue;
        }
        if Some(*indent) <= own {
            collecting = false;
            continue;
        }
        if let Some(item) = line.strip_prefix("- ") {
            values.push(unquote(item.trim()));
        }
    }
    values
}

/// A scalar that was quoted stays quoted-free; a condition that merely contains
/// quotes keeps them.
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    let wrapped = bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''));
    if wrapped {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

/// Every line of a job's body, so "this job reaches a registry" can be asked of
/// the steps rather than of the job's keys.
fn job_body(text: &str, job: &str) -> String {
    match block(text, &["jobs", job]) {
        Some(lines) => lines
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n"),
        None => String::new(),
    }
}

/// The lines belonging to the mapping at `path`, paired with their indentation.
/// Each name in `path` is entered as a key of the block before it.
fn block<'a>(text: &'a str, path: &[&str]) -> Option<Vec<(usize, &'a str)>> {
    let mut current: Vec<(usize, &str)> = text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| (line.len() - line.trim_start().len(), line.trim()))
        .collect();
    for name in path {
        current = child_block(&current, name)?;
    }
    Some(current)
}

/// The block owned by `name` inside `parent`: the lines after the key, up to the
/// next key at the same depth.
fn child_block<'a>(parent: &[(usize, &'a str)], name: &str) -> Option<Vec<(usize, &'a str)>> {
    let own = parent.iter().map(|(indent, _)| *indent).min()?;
    let mut out = Vec::new();
    let mut entered = false;
    for (indent, line) in parent {
        if *indent == own {
            if entered {
                break;
            }
            if line.split_once(':').map(|(key, _)| key.trim()) == Some(name) {
                entered = true;
            }
        } else if entered {
            out.push((*indent, *line));
        }
    }
    entered.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"
name: Release

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:
    inputs:
      dry_run:
        description: Build everything, publish nothing
        type: boolean
        default: true

jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --workspace
      - run: cargo xtask refresh-catalog --check
      - run: cargo xtask sync-version --check
      - run: cargo xtask check-package
      - run: cargo xtask check-docs
      - run: cargo xtask check-workflow
      - run: cargo xtask check-release "$GITHUB_REF_NAME"
      - run: cargo package --list --offline -p simllms | cargo xtask check-package --crate-listing -
      - run: cargo publish --dry-run -p simllms --allow-dirty

  wheels:
    needs: verify
    runs-on: ${{ matrix.runner }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - label: manylinux x86_64
            runner: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            container: quay.io/pypa/manylinux_2_28_x86_64:2026.10.10-1
          - label: manylinux aarch64
            runner: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            container: ghcr.io/rust-cross/manylinux_2_28-cross:aarch64
          - label: macOS x86_64
            runner: macos-latest
            target: x86_64-apple-darwin
          - label: macOS arm64
            runner: macos-latest
            target: aarch64-apple-darwin
          - label: Windows amd64
            runner: windows-latest
            target: x86_64-pc-windows-msvc
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          command: build
          manylinux: '2_28'
          target: ${{ matrix.target }}
          container: ${{ matrix.container }}
          args: --release --out dist
      - run: ls dist/*.whl | grep -q -- -abi3-

  distributions:
    needs: verify
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo package --no-verify --allow-dirty -p simllms
      - run: maturin sdist --out dist

  inventory:
    needs: [verify, wheels, distributions]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
      - run: cargo xtask check-artifacts dist --tag "$GITHUB_REF_NAME"

  seam:
    needs: [verify, inventory]
    runs-on: ubuntu-latest
    env:
      SIMLLMS_CRATE: ${{ needs.inventory.outputs.crate }}
      SIMLLMS_SDIST: ${{ needs.inventory.outputs.sdist }}
      SIMLLMS_WHEEL: ${{ needs.inventory.outputs.wheel }}
      SIMLLMS_RELEASE_TAG: ${{ needs.verify.outputs.tag }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
      - run: uv pip install "$SIMLLMS_WHEEL" pytest mypy
      - run: uv run --no-sync pytest tests
      - run: uv run --no-sync mypy python/simllms tests

  publish-python:
    needs: [verify, inventory, seam]
    runs-on: ubuntu-latest
    environment: release
    if: github.event_name == 'push' || github.event.inputs.dry_run != 'true'
    permissions:
      id-token: write
    steps:
      - uses: actions/download-artifact@v4
      - uses: pypa/gh-action-pypi-publish@release/v1
        with:
          repository: pypi
          skip-existing: true

  publish-crate:
    needs: [verify, inventory, seam]
    runs-on: ubuntu-latest
    environment: release
    if: github.event_name == 'push' || github.event.inputs.dry_run != 'true'
    steps:
      - uses: actions/download-artifact@v4
      - run: cargo publish --no-verify -p simllms
"#;

    const CI: &str = r#"
on:
  pull_request:
jobs:
  rust:
    steps:
      - run: cargo test --workspace
      - run: cargo xtask refresh-catalog --check
      - run: cargo xtask sync-version --check
      - run: cargo xtask check-package --crate-listing -
      - run: cargo xtask check-docs
      - run: cargo xtask check-workflow
  python:
    steps:
      - run: uv run --no-sync pytest tests
      - run: uv run --no-sync mypy python/simllms tests/test_client.py
"#;

    #[test]
    fn a_release_tag_is_v_and_three_numbers() {
        assert_eq!(tag_version("v0.1.0"), Some("0.1.0"));
        assert_eq!(tag_version("v10.20.30"), Some("10.20.30"));
        assert!(check_tag("v0.1.0").is_empty());
    }

    #[test]
    fn a_pre_release_or_build_suffix_is_refused_by_name() {
        for tag in ["v0.1.0-rc1", "v0.1.0+build5", "v1.0.0-beta.2"] {
            let problems = check_tag(tag);
            assert_eq!(problems.len(), 1, "{tag}");
            assert!(
                problems[0].contains("pre-release or build suffix"),
                "{}",
                problems[0]
            );
        }
    }

    #[test]
    fn a_malformed_tag_is_refused_by_name() {
        for (tag, rule) in [
            ("0.1.0", "starts with `v`"),
            ("v0.1", "three dot-separated numbers"),
            ("v0.1.0.2", "three dot-separated numbers"),
            ("vx.y.z", "three dot-separated numbers"),
        ] {
            let problems = check_tag(tag);
            assert!(problems[0].contains(rule), "{tag}: {:?}", problems[0]);
        }
    }

    #[test]
    fn the_tag_has_to_name_the_authoritative_version() {
        assert!(check_tag_version("v0.1.0", "0.1.0").is_empty());
        let problems = check_tag_version("v0.2.0", "0.1.0");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Authoritative Version"));
        assert!(problems[0].contains("0.2.0"));
        assert!(problems[0].contains("0.1.0"));
    }

    #[test]
    fn a_tag_of_the_wrong_shape_is_not_also_called_a_version_mismatch() {
        // One rule, named once: a shape failure has already been reported.
        assert!(check_tag_version("v0.1.0-rc1", "0.1.0").is_empty());
    }

    #[test]
    fn an_unreviewed_commit_is_refused_by_the_ancestor_rule() {
        assert!(check_ancestor("v0.1.0", true).is_empty());
        let problems = check_ancestor("v0.1.0", false);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("not an ancestor of `main`"));
    }

    #[test]
    fn the_changelog_has_to_date_the_version_it_publishes() {
        let text = "# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-16\n\n### Added\n";
        assert!(check_changelog(text, "0.1.0").is_empty());
        assert!(check_changelog(text, "0.2.0")[0].contains("no `## [0.2.0]` section"));
    }

    #[test]
    fn an_undated_or_placeholder_section_is_named() {
        let undated = "## [0.1.0]\n";
        assert!(check_changelog(undated, "0.1.0")[0].contains("without a date"));
        let placeholder = "## [0.1.0] - TBD\n";
        assert!(check_changelog(placeholder, "0.1.0")[0].contains("as `TBD`"));
    }

    #[test]
    fn the_intended_platform_set_is_exactly_five_wheels() {
        let names = artifacts(&[
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        assert_eq!(check_artifacts(&names, "0.1.0"), Vec::<String>::new());
    }

    #[test]
    fn a_missing_platform_fails_the_inventory() {
        let names = artifacts(&[
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("macOS arm64") && p.contains("not complete")),
            "{problems:?}"
        );
    }

    #[test]
    fn an_extra_platform_fails_the_inventory_too() {
        let names = artifacts(&[
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
            "simllms-0.1.0-cp310-abi3-win_arm64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("outside the intended platform set")),
            "{problems:?}"
        );
    }

    #[test]
    fn two_wheels_for_one_platform_are_a_second_wheel_nobody_intended() {
        let names = artifacts(&[
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp311-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("2 wheels for manylinux x86_64")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_per_interpreter_wheel_is_not_the_abi3_promise() {
        let names = artifacts(&[
            "simllms-0.1.0-cp312-cp312-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(
            problems.iter().any(|p| p.contains("one interpreter")),
            "{problems:?}"
        );
    }

    #[test]
    fn an_artifact_built_from_another_tree_is_not_this_release() {
        let names = artifacts(&[
            "simllms-0.2.0-cp310-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("originates from the binary")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_glibc_baseline_that_is_not_the_pin_is_named() {
        let names = artifacts(&[
            "simllms-0.1.0-cp310-abi3-manylinux_2_17_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(problems.iter().any(|p| p.contains("2_17")), "{problems:?}");
    }

    #[test]
    fn a_bare_linux_wheel_is_not_manylinux_and_says_so() {
        let names = artifacts(&[
            "simllms-0.1.0-cp310-abi3-linux_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]);
        let problems = check_artifacts(&names, "0.1.0");
        assert!(
            problems.iter().any(|p| p.contains("not manylinux")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_sdist_and_the_crate_are_one_each_and_of_the_tags_version() {
        let wheels = complete_wheels();
        let mut names: Vec<String> = wheels.iter().map(|w| w.to_string()).collect();
        names.push("simllms-0.1.0.tar.gz".to_string());
        names.push("simllms-0.2.0.crate".to_string());
        let problems = check_artifacts(&names, "0.1.0");
        assert!(problems.iter().any(|p| p.contains("crate version `0.2.0`")));

        let mut one_kind = wheels
            .iter()
            .map(|w| w.to_string())
            .chain([
                "simllms-0.1.0.tar.gz".to_string(),
                "simllms-0.1.0.tar.gz".to_string(),
                "simllms-0.1.0.crate".to_string(),
            ])
            .collect::<Vec<_>>();
        one_kind.push("SHA256SUMS".to_string());
        let problems = check_artifacts(&one_kind, "0.1.0");
        assert!(
            problems.iter().any(|p| p.contains("2 sdists")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("unexplained file")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_release_line_triggers_on_a_tag_and_a_dispatch_only() {
        assert!(check_triggers(RELEASE).is_empty());
    }

    #[test]
    fn a_comment_or_pull_request_trigger_is_refused() {
        for trigger in ["pull_request", "issue_comment", "schedule", "workflow_run"] {
            let text = RELEASE.replace(
                "  workflow_dispatch:",
                &format!("  {trigger}:\n  workflow_dispatch:"),
            );
            let problems = check_triggers(&text);
            assert!(
                problems.iter().any(|p| p.contains(trigger)),
                "{trigger}: {problems:?}"
            );
        }
    }

    #[test]
    fn a_push_trigger_without_a_tag_filter_is_refused() {
        let text = RELEASE.replace("    tags:\n      - 'v*'\n", "");
        let problems = check_triggers(&text);
        assert!(
            problems.iter().any(|p| p.contains("no `v*` tag filter")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_rehearsal_is_asked_for_by_name_and_a_tag_is_never_an_input() {
        assert!(check_dispatch(RELEASE).is_empty());
        let no_inputs = RELEASE.replace(
            "    inputs:\n      dry_run:\n        description: Build everything, publish nothing\n        type: boolean\n        default: true\n",
            "",
        );
        let problems = check_dispatch(&no_inputs);
        assert!(problems[0].contains("declares no `inputs`"), "{problems:?}");

        let tagged = RELEASE.replace(
            "      dry_run:",
            "      tag:\n        type: string\n      dry_run:",
        );
        let problems = check_dispatch(&tagged);
        assert!(problems[0].contains("take a `tag`"), "{problems:?}");
    }

    #[test]
    fn the_matrix_names_every_platform_the_pin_and_the_abi() {
        assert!(check_matrix(RELEASE).is_empty());
    }

    #[test]
    fn an_unpinned_or_defaulted_base_image_is_refused() {
        let drifting = RELEASE.replace(
            "container: quay.io/pypa/manylinux_2_28_x86_64:2026.10.10-1",
            "container: quay.io/pypa/manylinux_2_28_x86_64:latest",
        );
        let problems = check_matrix(&drifting);
        assert!(
            problems.iter().any(|p| p.contains("is not pinned")),
            "{problems:?}"
        );

        let defaulted = RELEASE.replace(
            "          - label: manylinux x86_64\n            runner: ubuntu-latest\n            target: x86_64-unknown-linux-gnu\n            container: quay.io/pypa/manylinux_2_28_x86_64:2026.10.10-1\n",
            "",
        );
        let problems = check_matrix(&defaulted);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("x86_64-unknown-linux-gnu") && p.contains("not built")),
            "{problems:?}"
        );
    }

    #[test]
    fn an_emulated_aarch64_build_is_refused() {
        let emulated = RELEASE.replace(
            "  wheels:\n    needs: verify\n",
            "  wheels:\n    needs: verify\n    steps:\n      - run: apt-get install -y qemu-user-static\n",
        );
        let problems = check_matrix(&emulated);
        assert!(problems.iter().any(|p| p.contains("qemu")), "{problems:?}");
    }

    #[test]
    fn publication_is_a_separate_job_behind_an_environment_and_a_visible_skip() {
        assert!(check_separation(RELEASE).is_empty());
    }

    #[test]
    fn a_publisher_that_computes_instead_of_consuming_artifacts_is_named() {
        let merged = RELEASE.replace(
            "  publish-crate:\n    needs: [verify, inventory, seam]\n    runs-on: ubuntu-latest\n    environment: release\n    if: github.event_name == 'push' || github.event.inputs.dry_run != 'true'\n",
            "  publish-crate:\n    runs-on: ubuntu-latest\n    if: github.event_name == 'push' || github.event.inputs.dry_run != 'true'\n",
        );
        let problems = check_separation(&merged);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("no GitHub environment") && p.contains("publish-crate")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("needs nothing")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_publisher_that_ignores_the_rehearsal_is_named() {
        let silent = RELEASE.replace(
            "    environment: release\n    if: github.event_name == 'push' || github.event.inputs.dry_run != 'true'\n    permissions:",
            "    environment: release\n    permissions:",
        );
        let problems = check_separation(&silent);
        assert!(
            problems.iter().any(|p| p.contains("publish-python")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_packager_dry_run_is_not_mistaken_for_a_publisher() {
        // The verify job rehearses `cargo publish`, and owes no gate for it.
        assert!(
            check_separation(RELEASE)
                .iter()
                .all(|p| !p.contains("`verify`"))
        );

        // The same line without the rehearsal publishes, from a job that stands
        // in front of nothing.
        let ungated = RELEASE.replace(
            "      - run: cargo publish --dry-run -p simllms --allow-dirty\n",
            "      - run: cargo publish --no-verify -p simllms\n",
        );
        let problems = check_separation(&ungated);
        assert!(
            problems.iter().any(|p| p.contains("`verify`")),
            "{problems:?}"
        );
    }

    #[test]
    fn every_pull_request_gate_runs_again_in_the_release() {
        assert!(check_gate_parity(CI, RELEASE).is_empty());
    }

    #[test]
    fn a_gate_that_only_exists_in_the_pull_request_path_is_named() {
        let drifted = RELEASE.replace("      - run: cargo xtask check-docs\n", "");
        let problems = check_gate_parity(CI, &drifted);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("cargo xtask check-docs")),
            "{problems:?}"
        );

        let untested = RELEASE.replace("      - run: cargo test --workspace\n", "");
        let problems = check_gate_parity(CI, &untested);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("cargo test --workspace")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_new_gate_in_the_pull_request_gate_becomes_a_release_requirement() {
        // Nothing was edited in the release file: the requirement is derived.
        let grown = format!("{CI}\n      - run: cargo xtask check-new-thing\n");
        let problems = check_gate_parity(&grown, RELEASE);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("cargo xtask check-new-thing")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_pull_request_gate_is_not_rust_only() {
        // The Python half of the family runs in the seam job, against the built
        // artifacts. A release line that rehearses the packaging and skips the
        // suites is the thing this rule exists to catch.
        for (line, gate) in [
            (
                "      - run: uv run --no-sync pytest tests\n",
                "uv run --no-sync pytest",
            ),
            (
                "      - run: uv run --no-sync mypy python/simllms tests\n",
                "uv run --no-sync mypy",
            ),
        ] {
            let drifted = RELEASE.replace(line, "");
            let problems = check_gate_parity(CI, &drifted);
            assert!(
                problems.iter().any(|p| p.contains(gate)),
                "`{gate}` must be required: {problems:?}"
            );
        }
    }

    #[test]
    fn a_step_titled_as_a_gate_does_not_run_it() {
        // `code()` keeps `- name:` lines, because a name is not a comment; were
        // the scan satisfied by one, a step could claim a gate it never runs.
        let titled = RELEASE.replace(
            "      - run: cargo xtask check-docs\n",
            "      - name: cargo xtask check-docs\n        run: echo ok\n",
        );
        let problems = check_gate_parity(CI, &titled);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("cargo xtask check-docs")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_release_line_runs_its_own_verdicts() {
        assert!(check_invoked_gates(RELEASE).is_empty());

        // Deleting a step leaves a file that is still well-formed, so parity with
        // the pull-request gate cannot notice: the tag would go out ungated by a
        // rule that exists only as a function nobody calls.
        let ungated = RELEASE.replace(
            "      - run: cargo xtask check-release \"$GITHUB_REF_NAME\"\n",
            "",
        );
        let problems = check_invoked_gates(&ungated);
        assert!(
            problems.iter().any(|p| p.contains("check-release")),
            "{problems:?}"
        );

        let uninspected = RELEASE.replace(
            "      - run: cargo xtask check-artifacts dist --tag \"$GITHUB_REF_NAME\"\n",
            "",
        );
        let problems = check_invoked_gates(&uninspected);
        assert!(
            problems.iter().any(|p| p.contains("check-artifacts")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_seam_without_its_tag_or_its_wheel_tests_nothing() {
        // Both absence paths are silent: no tag means every release assertion
        // skips, and no named wheel means the import resolves into this checkout.
        let untagged = RELEASE.replace(
            "      SIMLLMS_RELEASE_TAG: ${{ needs.verify.outputs.tag }}\n",
            "",
        );
        let problems = check_invoked_gates(&untagged);
        assert!(
            problems.iter().any(|p| p.contains("SIMLLMS_RELEASE_TAG")),
            "{problems:?}"
        );

        let source_installed =
            RELEASE.replace("uv pip install \"$SIMLLMS_WHEEL\"", "uv pip install .");
        let problems = check_invoked_gates(&source_installed);
        assert!(
            problems.iter().any(|p| p.contains("SIMLLMS_WHEEL")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_manylinux_row_with_no_named_image_is_a_row_with_no_baseline() {
        // Both manylinux rows name an image. Drop one image and the row is still
        // syntactically fine — it just builds inside the builder's default, and
        // the wheel would carry a baseline nobody wrote down.
        let defaulted = RELEASE.replace(
            "            container: ghcr.io/rust-cross/manylinux_2_28-cross:aarch64\n",
            "",
        );
        let problems = check_matrix(&defaulted);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("manylinux platforms but only 1 base image")),
            "{problems:?}"
        );

        let off_baseline = RELEASE.replace(
            "container: quay.io/pypa/manylinux_2_28_x86_64:2026.10.10-1",
            "container: quay.io/pypa/manylinux_2_34_x86_64:2026.10.10-1",
        );
        let problems = check_matrix(&off_baseline);
        assert!(
            problems.iter().any(|p| p.contains(MANYLINUX)),
            "{problems:?}"
        );
    }

    #[test]
    fn the_pinned_baseline_and_the_inspected_wheel_tag_are_one_number() {
        // `MANYLINUX` is what the workflow has to name and `GLIBC_BASELINE` is
        // what a wheel's platform tag has to carry. Two literals kept equal only
        // by a comment drift, so this is the thing that holds them.
        assert!(MANYLINUX.ends_with(GLIBC_BASELINE));
        assert_eq!(GLIBC_BASELINE, "2_28");
    }

    #[test]
    fn a_comment_never_satisfies_a_rule_about_what_a_release_does() {
        let commented = RELEASE.replace(
            "      - run: cargo xtask check-docs\n",
            "      # cargo xtask check-docs\n",
        );
        assert!(!check_gate_parity(CI, &commented).is_empty());
        let mentioned = RELEASE.replace(
            "  verify:\n    runs-on: ubuntu-latest\n",
            "  verify:\n    runs-on: ubuntu-latest\n    # qemu is not used here\n",
        );
        assert!(
            check_matrix(&mentioned).is_empty(),
            "a comment is not a run step"
        );
    }

    #[test]
    fn the_shallow_yaml_read_sees_the_blocks_it_is_asked_to_see() {
        assert_eq!(
            mapping_keys(RELEASE, &["on"]),
            vec!["push", "workflow_dispatch"]
        );
        assert_eq!(
            mapping_keys(RELEASE, &["jobs"]),
            vec![
                "verify",
                "wheels",
                "distributions",
                "inventory",
                "seam",
                "publish-python",
                "publish-crate"
            ]
        );
        assert_eq!(
            mapping_keys(RELEASE, &["on", "workflow_dispatch", "inputs"]),
            vec!["dry_run"]
        );
        assert_eq!(
            scalar(RELEASE, &["on", "push"], "tags"),
            vec!["v*".to_string()]
        );
        assert_eq!(
            scalar(RELEASE, &["jobs", "publish-crate"], "needs"),
            vec![
                "verify".to_string(),
                "inventory".to_string(),
                "seam".to_string()
            ]
        );
        assert_eq!(
            scalar(RELEASE, &["jobs", "publish-python"], "environment"),
            vec!["release".to_string()]
        );
        assert!(scalar(RELEASE, &["jobs", "verify"], "environment").is_empty());
        assert!(block(RELEASE, &["jobs", "nope"]).is_none());
        // A condition keeps the quotes inside it; only a wrapped scalar loses them.
        assert_eq!(
            scalar(RELEASE, &["jobs", "publish-crate"], "if")[0],
            "github.event_name == 'push' || github.event.inputs.dry_run != 'true'"
        );
    }

    #[test]
    fn the_flat_read_finds_a_key_at_whatever_depth_it_sits() {
        // Two matrix rows name an image; the step that passes it through is a
        // template, and a template is not an image anybody could pin.
        let containers = values_of_key(RELEASE, "container");
        assert_eq!(
            containers,
            vec![
                "quay.io/pypa/manylinux_2_28_x86_64:2026.10.10-1".to_string(),
                "ghcr.io/rust-cross/manylinux_2_28-cross:aarch64".to_string(),
            ]
        );
        assert_eq!(
            values_of_key(RELEASE, "manylinux"),
            vec!["2_28".to_string()]
        );
    }

    #[test]
    fn the_platform_tag_reduces_to_the_platform_the_set_is_written_in() {
        assert_eq!(identity("macosx_11_0_arm64"), "macos_arm64");
        assert_eq!(identity("macosx_12_0_arm64"), "macos_arm64");
        assert_eq!(identity("manylinux_2_28_x86_64"), "linux_x86_64");
        assert_eq!(identity("win_amd64"), "windows_amd64");
        assert_eq!(identity("musllinux_1_2_aarch64"), "musl_aarch64");
        assert_eq!(identity("whatever"), "other_whatever");
        assert_eq!(glibc_baseline("manylinux_2_28_aarch64"), Some("2_28"));
        assert_eq!(glibc_baseline("manylinux_2_17_x86_64"), Some("2_17"));
        assert_eq!(glibc_baseline("macosx_11_0_arm64"), None);
    }

    fn complete_wheels() -> [&'static str; 5] {
        [
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-manylinux_2_28_aarch64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_x86_64.whl",
            "simllms-0.1.0-cp310-abi3-macosx_11_0_arm64.whl",
            "simllms-0.1.0-cp310-abi3-win_amd64.whl",
        ]
    }

    /// A release directory holds the five wheels, one sdist and one crate; a
    /// checksum file is written alongside them and is not evidence of anything,
    /// so the fixture adds what a real run produces.
    fn artifacts(wheels: &[&str]) -> Vec<String> {
        wheels
            .iter()
            .map(|name| name.to_string())
            .chain([
                "simllms-0.1.0.tar.gz".to_string(),
                "simllms-0.1.0.crate".to_string(),
            ])
            .collect()
    }
}
