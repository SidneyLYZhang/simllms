"""S1 at the seam: the release artifacts, as an installer receives them.

`test_package_inventory.py` opens the archives. This file imports them. The
difference is what the release line is actually promising — that a wheel built
by the matrix, installed into a venv that never saw this checkout, reports the
version the tag announced, carries its attribution, refuses outside its
Capabilities, and renders a request without a credential.

This suite is dormant unless the release names the tag to compare against:

    SIMLLMS_RELEASE_TAG=v0.1.0 pytest tests

Without it the checks skip, because a pull-request run has no tag and inventing
one would let the release pass by comparing the artifacts to nothing.
"""

from __future__ import annotations

import os
import re
from email.message import Message
from email.parser import BytesParser
from email.policy import default
from importlib import metadata
from pathlib import Path

import pytest

import simllms
from simllms import CapabilityError, Client, SimLLMError, _core

PYTHON_DIR = Path(__file__).resolve().parents[1]
SOURCE_PACKAGE = (PYTHON_DIR / "python" / "simllms").resolve()

RELEASE_TAG = os.environ.get("SIMLLMS_RELEASE_TAG", "")

LICENSES = ("LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES.md")
SUMMARY = "Unified client for multiple LLM providers"
ENGLISH_README_LINE = "One client shape for every major LLM provider"

# The extension must be the compiled module the wheel installed, not this
# checkout's stub: that is what makes the reported version the binary's.
EXTENSIONS = (".so", ".pyd", ".dylib")

pytestmark = pytest.mark.skipif(
    not RELEASE_TAG,
    reason="no tag to compare against: set SIMLLMS_RELEASE_TAG to the release under test",
)


def tag_version() -> str:
    """`vX.Y.Z` without the `v` — the form every artifact and every module carries."""
    assert re.fullmatch(
        r"v\d+\.\d+\.\d+", RELEASE_TAG
    ), f"{RELEASE_TAG!r} is not a release tag, so nothing here can be compared to it"
    return RELEASE_TAG[1:]


def distribution_files() -> list[str]:
    files = metadata.files("simllms")
    assert files, "the installed distribution reports no files: it was not installed"
    return [str(entry) for entry in files]


def installed_metadata() -> Message:
    """The installed `METADATA`, read as bytes and parsed. `importlib.metadata`'s
    own view exposes the headers it knows and hides the description body, and the
    body is one of the claims being checked here."""
    files = metadata.files("simllms")
    assert files, "the installed distribution reports no files: it was not installed"
    record = next(entry for entry in files if str(entry).endswith(".dist-info/METADATA"))
    return BytesParser(policy=default).parsebytes(Path(record.locate()).read_bytes())


def module_file(module: object) -> Path:
    """The file a module was loaded from, via `getattr` because a stub need not
    declare it and a compiled extension may report none."""
    path = getattr(module, "__file__", None)
    assert isinstance(path, str) and path, "the module reports no file to load from"
    return Path(path)


def test_the_installed_package_is_the_one_the_wheel_carried() -> None:
    assert module_file(simllms).parent.resolve() != SOURCE_PACKAGE, (
        "the tests are importing this checkout rather than the installed "
        "distribution, so they would pass whatever the release shipped"
    )


def test_the_announced_version_is_the_installed_version() -> None:
    assert simllms.__version__ == tag_version(), (
        f"the tag says {RELEASE_TAG} and the package says {simllms.__version__}"
    )
    assert metadata.version("simllms") == tag_version()


def test_the_version_comes_out_of_the_binary() -> None:
    # A Python-side literal would agree with the tag on the day it was written
    # and drift silently afterwards. The compiled module is the only owner.
    assert _core.__version__ == tag_version()
    assert simllms.__version__ == _core.__version__
    assert module_file(_core).suffix in EXTENSIONS, (
        f"{module_file(_core)} is not a compiled extension, so its version "
        "could have been written by hand"
    )


def test_an_installed_distribution_holds_its_licenses_and_attribution() -> None:
    files = distribution_files()
    for name in LICENSES:
        assert any(entry.endswith(f"licenses/{name}") for entry in files), (
            f"an installer cannot find {name}: PEP 639 puts it under "
            "`*.dist-info/licenses/`, and nothing else in the distribution is "
            "obliged to carry it"
        )
    assert not [entry for entry in files if entry.endswith(".pyc")], files


def test_the_installed_metadata_describes_the_package_not_a_provider() -> None:
    message = installed_metadata()
    assert message["Summary"] == SUMMARY
    assert message["License-Expression"] == "MIT OR Apache-2.0"
    body = message.get_payload()
    assert isinstance(body, str)
    assert ENGLISH_README_LINE in body, "the registry page renders the English README"


def test_the_gate_refuses_a_text_only_model_before_the_network() -> None:
    # `deepseek-v4-pro` is Known as text-only, and the base_url is a dead port:
    # had the Gate let the call through, the error would be a TransportError.
    client = Client(
        "deepseek",
        "deepseek-v4-pro",
        api_key="sk-test",
        base_url="http://127.0.0.1:1",
    )
    messages = [
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "what is this?"},
                {"type": "image_url", "url": "https://example.com/cat.png"},
            ],
        }
    ]

    with pytest.raises(CapabilityError) as excinfo:
        client.chat(messages)

    assert excinfo.value.model == "deepseek-v4-pro"
    assert excinfo.value.missing == "ImageInput"
    assert isinstance(excinfo.value, SimLLMError)


def test_the_request_debugger_renders_the_body_without_the_credential() -> None:
    client = Client(
        "deepseek",
        "deepseek-v4-pro",
        api_key="sk-test",
        base_url="http://127.0.0.1:1",
    )
    debug = client.debug_request(["hi"], options={"max_tokens": 32})

    assert "deepseek-v4-pro" in debug
    assert "max_tokens" in debug
    assert "sk-test" not in debug, "a rendered request is something people paste into issues"
