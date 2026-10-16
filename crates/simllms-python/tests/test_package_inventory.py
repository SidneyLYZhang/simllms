"""S1: the published artifacts, judged by what is inside them.

A registry never tells you a license went missing — the wheel simply has no
license, and nobody who installs it can find the attribution obligation for the
vendored Catalog Snapshot. So the checks run on the unpacked artifact, the same
way, whether it was packed locally, by a dry run, or for a real release.

The artifacts are built outside pytest, then handed in:

    cargo package --no-verify --offline -p simllms
    maturin build --out target/artifacts
    maturin sdist --out target/artifacts

`SIMLLMS_CRATE`, `SIMLLMS_WHEEL` and `SIMLLMS_SDIST` name them; without those,
the default output directories are searched. Nothing built yet means the checks
skip rather than pass.
"""

from __future__ import annotations

import os
import re
import tarfile
import zipfile
from collections.abc import Iterable
from email.message import Message
from email.parser import BytesParser
from email.policy import default
from pathlib import Path

import pytest

PYTHON_DIR = Path(__file__).resolve().parents[1]
REPO = PYTHON_DIR.parents[1]

LICENSES = ("LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md")
CARRIED = ("README.en.md", "CHANGELOG.md", *LICENSES)

# crates.io takes five keywords, so this side is the short list; PyPI carries
# all of them. Which provider names make the cut is a decision, not a default.
CROSS_PROVIDER_KEYWORD = "openai-compatible"
CRATE_KEYWORDS = ("llm", CROSS_PROVIDER_KEYWORD, "deepseek", "moonshot", "qwen")
CRATE_SOURCE_DIRS = ("src", "examples")
# The whitelist names files, never a directory: cargo's `include` reaches into a
# listed directory recursively and does not ask version control about it, so a
# bare `src` would pack an ignored neighbour of the sources.
CRATE_SOURCE_GLOBS = ("src/*.rs", "examples/*.rs")

# What a working directory accumulates and a release has no business carrying.
# The release job plants a marker inside each of these before packing, so the
# assertion below is not a report that nothing happened to be there — it is a
# report that the packagers kept what is definitely there out.
WORKING_DIRECTORY_NAMES = ("target", "dist", "build", ".venv", "venv", "__pycache__", ".git")
WORKING_DIRECTORY_SUFFIXES = (".pyc", ".pyo")

# The one sentence both registries show, and the line only the English README
# has: the summary must not name a provider, and the page must render English.
SUMMARY = "Unified client for multiple LLM providers"
ENGLISH_README_LINE = "One client shape for every major LLM provider"


def derived_version() -> str:
    """The Authoritative Version's written copy, which the build reads."""
    manifest = (PYTHON_DIR / "Cargo.toml").read_text(encoding="utf-8")
    found = re.search(r'^version = "([^"]+)"', manifest, re.MULTILINE)
    assert found, "the binding crate's manifest must carry the derived version"
    return found.group(1)


def toml_array(manifest: str, key: str) -> list[str]:
    """The strings of one array in a packed manifest.

    cargo rewrites the manifest as it packs — workspace inheritance resolved,
    the array laid out its own way — so the shape is not ours to assume and the
    values are read back rather than matched as text.
    """
    found = re.search(rf"^{key} = \[(.*?)\]", manifest, re.MULTILINE | re.DOTALL)
    assert found, f"the packed manifest declares no `{key}` array"
    return re.findall(r'"([^"]*)"', found.group(1))


def artifact(env: str, patterns: tuple[str, ...]) -> Path:
    explicit = os.environ.get(env)
    if explicit:
        path = Path(explicit)
        if not path.is_file():
            pytest.fail(f"{env} points at {path}, which is not a file")
        return path
    for pattern in patterns:
        found = sorted(REPO.glob(pattern))
        if found:
            return found[-1]
    pytest.skip(f"no artifact to inspect: build one, or name it in {env}")


@pytest.fixture(scope="module")
def crate() -> Path:
    return artifact(
        "SIMLLMS_CRATE",
        (
            "target/package/simllms-*.crate",
            "crates/simllms-python/target/package/simllms-*.crate",
        ),
    )


@pytest.fixture(scope="module")
def wheel() -> Path:
    return artifact("SIMLLMS_WHEEL", ("crates/simllms-python/target/artifacts/*.whl",))


@pytest.fixture(scope="module")
def sdist() -> Path:
    return artifact("SIMLLMS_SDIST", ("crates/simllms-python/target/artifacts/*.tar.gz",))


def tar_contents(path: Path) -> dict[str, bytes]:
    """An archive's files, keyed by path with the `<name>-<version>/` root removed.
    The version is one of the things under test, so the prefix is stripped rather
    than assumed."""
    contents: dict[str, bytes] = {}
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            if not member.isfile():
                continue
            handle = archive.extractfile(member)
            assert handle is not None
            _, _, relative = member.name.partition("/")
            contents[relative] = handle.read()
    return contents


def wheel_contents(path: Path) -> tuple[list[str], bytes]:
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        metadata = archive.read(next(name for name in names if name.endswith(".dist-info/METADATA")))
    return names, metadata


def wheel_metadata(path: Path) -> tuple[list[str], Message]:
    names, metadata = wheel_contents(path)
    return names, BytesParser(policy=default).parsebytes(metadata)


def license_directory(names: list[str]) -> str:
    return next(name for name in names if name.endswith(".dist-info/METADATA")).rsplit("/", 1)[0]


def test_crate_carries_its_licenses_attribution_readme_and_changelog(crate: Path) -> None:
    files = tar_contents(crate)
    for required in CARRIED:
        assert required in files, f"the crate has no {required}"
    assert files["src/lib.rs"], "the crate must contain its own source"
    assert "examples/deepseek_chat.rs" in files, "the examples are documentation a caller reads"


def test_crate_is_named_for_the_derived_version(crate: Path) -> None:
    version = derived_version()
    assert crate.name == f"simllms-{version}.crate", crate.name
    manifest = tar_contents(crate)["Cargo.toml"].decode("utf-8")
    assert f'version = "{version}"' in manifest, "cargo's normalized manifest lost the version"


def test_crate_manifest_points_the_registry_at_the_english_readme(crate: Path) -> None:
    manifest = tar_contents(crate)["Cargo.toml"].decode("utf-8")
    assert 'readme = "README.en.md"' in manifest, manifest
    assert f'description = "{SUMMARY}"' in manifest
    assert 'license = "MIT OR Apache-2.0"' in manifest, "workspace inheritance must resolve"
    assert "license-file" not in manifest, "cargo takes either `license` or `license-file`"
    assert sorted(toml_array(manifest, "keywords")) == sorted(CRATE_KEYWORDS), manifest


def test_the_packed_whitelist_promises_everything_the_release_has_to_carry(crate: Path) -> None:
    # Read from the manifest inside the archive: what the artifact itself
    # promises is what a registry user receives, so a whitelist that quietly
    # drops one of these files goes red even when the archive was packed from a
    # directory that still holds it.
    include = toml_array(tar_contents(crate)["Cargo.toml"].decode("utf-8"), "include")
    for required in (*CARRIED, *CRATE_SOURCE_GLOBS):
        assert required in include, f"`[package] include` leaves out `{required}`"


def test_the_crate_holds_nothing_the_whitelist_never_asked_for(crate: Path) -> None:
    allowed = set(CARRIED) | set(CRATE_SOURCE_DIRS) | {
        "Cargo.toml",
        "Cargo.toml.orig",
        "Cargo.lock",
        ".cargo_vcs_info.json",
    }
    extra = sorted({name.partition("/")[0] for name in tar_contents(crate)} - allowed)
    assert extra == [], f"the crate carries {extra}"


def test_wheel_exposes_licenses_and_attribution_where_a_spec_says(wheel: Path) -> None:
    names, metadata = wheel_metadata(wheel)
    licenses = f"{license_directory(names)}/licenses"
    for license_file in LICENSES:
        assert f"{licenses}/{license_file}" in names, (
            f"an installed distribution must hold {license_file}, and PEP 639 puts it under "
            f"{licenses}/"
        )
    assert sorted(metadata.get_all("License-File") or []) == sorted(LICENSES)
    assert metadata["License-Expression"] == "MIT OR Apache-2.0"
    classifiers = metadata.get_all("Classifier") or []
    assert not [c for c in classifiers if c.startswith("License ::")], (
        "PEP 639 forbids a License :: classifier next to an SPDX expression"
    )


def test_wheel_says_what_it_is_in_one_agnostic_sentence(wheel: Path) -> None:
    _, metadata = wheel_metadata(wheel)
    assert metadata["Summary"] == SUMMARY
    keywords = (metadata["Keywords"] or "").split(",")
    assert "openai-compatible" in keywords, keywords
    for provider in ("deepseek", "moonshot", "qwen", "dashscope", "zhipu", "openrouter"):
        assert provider in keywords, f"{provider} belongs in the keywords, not in the summary"
    body = metadata.get_payload()
    assert isinstance(body, str)
    assert ENGLISH_README_LINE in body, "a registry page renders the English README"


def test_wheel_installs_the_package_it_claims(wheel: Path) -> None:
    names, metadata = wheel_metadata(wheel)
    assert metadata["Version"] == derived_version()
    assert wheel.name.startswith(f"simllms-{derived_version()}-"), wheel.name
    assert "simllms/__init__.py" in names, names
    assert any(name.startswith("simllms/_core.") for name in names), names


def test_wheel_holds_only_the_installed_package(wheel: Path) -> None:
    names, _ = wheel_contents(wheel)
    assert not [name for name in names if name.endswith(".pyc") or "__pycache__" in name], names
    loose = [
        name for name in names if Path(name).name in CARRIED and "/licenses/" not in name
    ]
    assert loose == [], f"the wheel carries {loose} where no spec puts them"


def test_sdist_carries_every_file_it_declares(sdist: Path) -> None:
    files = tar_contents(sdist)
    for required in (*CARRIED, "pyproject.toml"):
        assert required in files, f"the source distribution has no {required}"
    assert any(name.endswith("python/simllms/__init__.py") for name in files), sorted(files)
    assert any(name.endswith("Cargo.toml") for name in files), sorted(files)


def working_directories_inside(names: Iterable[str]) -> list[str]:
    return sorted(
        name
        for name in names
        if any(segment in WORKING_DIRECTORY_NAMES for segment in name.split("/"))
        or name.endswith(WORKING_DIRECTORY_SUFFIXES)
    )


def test_no_artifact_carries_a_working_directory(crate: Path, wheel: Path, sdist: Path) -> None:
    """Ignored directories provably cannot enter, at any depth, in any archive.

    The sdist embeds a copy of the core crate, so one marker under either crate
    is enough to make this a release defect rather than a packaging curiosity.
    """
    offenders = working_directories_inside(tar_contents(crate))
    assert offenders == [], f"the crate carries {offenders}"
    offenders = working_directories_inside(tar_contents(sdist))
    assert offenders == [], f"the source distribution carries {offenders}"
    names, _ = wheel_contents(wheel)
    offenders = working_directories_inside(names)
    assert offenders == [], f"the wheel carries {offenders}"


def test_the_copies_inside_the_artifacts_are_the_root_files(wheel: Path, crate: Path) -> None:
    """A package carries copies of root files; an artifact is where that is proven."""
    names, _ = wheel_contents(wheel)
    licenses = f"{license_directory(names)}/licenses"
    with zipfile.ZipFile(wheel) as archive:
        for name in LICENSES:
            assert archive.read(f"{licenses}/{name}") == (REPO / name).read_bytes(), name
    files = tar_contents(crate)
    for name, body in files.items():
        if name in CARRIED:
            assert body == (REPO / name).read_bytes(), f"the crate's {name} drifted"
