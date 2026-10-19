"""The typed surface refuses mistakes statically (spec #23 / ticket #25).

Each case runs mypy on a fresh snippet in a scratch directory and asserts the
exact mistake is diagnosed — so a future edit that widens the surface back to
``dict[str, Any]`` or drops a field type fails here, not at some user's
runtime. The snippets live outside the repo, so neither the project mypy run
nor the packaging checks ever see them.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest

CASES = [
    (
        "an unknown override key",
        "from simllms import CapabilityOverrides\n"
        "CapabilityOverrides(image_inputn=True)\n",
        "image_inputn",
    ),
    (
        "a wrong override value type",
        "from simllms import CapabilityOverrides\n"
        'CapabilityOverrides(image_input="yes")\n',
        'Argument "image_input"',
    ),
    (
        "a dict passed where the typed object is required",
        "from simllms import Client\n"
        'Client("deepseek", "m", api_key="sk", capability_overrides={"image_input": True})\n',
        'Argument "capability_overrides"',
    ),
]


def run_mypy(snippet: str, tmp_path: Path) -> subprocess.CompletedProcess[str]:
    scratch = tmp_path / "snippet.py"
    scratch.write_text(snippet, encoding="utf-8")
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "mypy",
            "--python-executable",
            sys.executable,
            "--no-error-summary",
            "--cache-dir",
            str(tmp_path / ".mypy_cache"),
            str(scratch),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=120,
    )


@pytest.mark.parametrize(
    ("name", "snippet", "needle"),
    CASES,
    ids=[name for name, _, _ in CASES],
)
def test_mypy_rejects(name: str, snippet: str, needle: str, tmp_path: Path) -> None:
    completed = run_mypy(snippet, tmp_path)

    assert completed.returncode != 0, f"mypy accepted {name}:\n{completed.stdout}"
    assert needle in completed.stdout, completed.stdout


def test_mypy_accepts_the_typed_surface(tmp_path: Path) -> None:
    completed = run_mypy(
        "from simllms import CapabilityOverrides, Client\n"
        'client = Client("deepseek", "m", api_key="sk", '
        "capability_overrides=CapabilityOverrides(image_input=True))\n"
        "assert client\n",
        tmp_path,
    )

    assert completed.returncode == 0, completed.stdout
