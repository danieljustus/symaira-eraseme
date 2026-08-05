"""Packaging regression tests for issue #615.

The broker registry data tree (``registry/brokers``, ``registry/laws``,
``registry/schemas``, ``registry/templates``) must be shipped inside the
built wheel and sdist, otherwise every pip/Homebrew install crashes with
``RegistryError('Could not find registry directory')`` on ``brokers list``
and campaign creation.

These tests build the artifacts with ``uv build``, inspect the wheel/sdist
contents, and run ``brokers list`` from a clean virtualenv that only
contains the wheel.  They skip when the ``uv`` binary is unavailable.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]

pytestmark = pytest.mark.skipif(
    shutil.which("uv") is None,
    reason="uv is required to build the wheel and provision the test venv",
)


def _run(
    cmd: list[str],
    cwd: Path,
    *,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=str(cwd),
        capture_output=True,
        text=True,
        env=env,
        timeout=600,
        check=False,
    )


@pytest.fixture(scope="module")
def built_artifacts(tmp_path_factory: pytest.TempPathFactory) -> tuple[Path, Path]:
    """Build the wheel and sdist once per test module."""
    dist_dir = tmp_path_factory.mktemp("dist")
    result = _run(["uv", "build", "--out-dir", str(dist_dir)], cwd=REPO_ROOT)
    assert result.returncode == 0, f"uv build failed:\n{result.stdout}\n{result.stderr}"
    wheels = sorted(dist_dir.glob("*.whl"))
    sdists = sorted(dist_dir.glob("*.tar.gz"))
    assert wheels, f"uv build produced no wheel in {dist_dir}"
    assert sdists, f"uv build produced no sdist in {dist_dir}"
    return wheels[-1], sdists[-1]


@pytest.fixture(scope="module")
def wheel_only_venv(
    tmp_path_factory: pytest.TempPathFactory,
    built_artifacts: tuple[Path, Path],
) -> Path:
    """A clean virtualenv that only contains the built wheel and its deps."""
    wheel, _sdist = built_artifacts
    venv_dir = tmp_path_factory.mktemp("venv")

    result = _run(["uv", "venv", str(venv_dir)], cwd=REPO_ROOT)
    assert result.returncode == 0, f"uv venv failed:\n{result.stdout}\n{result.stderr}"

    venv_python = venv_dir / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    result = _run(
        ["uv", "pip", "install", "--python", str(venv_python), str(wheel)],
        cwd=REPO_ROOT,
    )
    assert result.returncode == 0, f"uv pip install failed:\n{result.stdout}\n{result.stderr}"
    return venv_dir


def test_wheel_and_sdist_contain_broker_registry_data(
    built_artifacts: tuple[Path, Path],
) -> None:
    """Built artifacts must carry the registry data tree (regression for #615)."""
    wheel, sdist = built_artifacts

    with zipfile.ZipFile(wheel) as zf:
        wheel_names = zf.namelist()

    broker_yamls = [
        n
        for n in wheel_names
        if n.startswith("symeraseme/registry_data/brokers/") and n.endswith(".yaml")
    ]
    assert broker_yamls, (
        "wheel is missing symeraseme/registry_data/brokers/*.yaml — "
        "registry data is not force-included into the wheel (#615)"
    )
    assert "symeraseme/registry_data/schemas/broker.schema.json" in wheel_names, (
        "wheel is missing symeraseme/registry_data/schemas/broker.schema.json (#615)"
    )
    assert "symeraseme/registry_data/laws/gdpr-art17.de.md.j2" in wheel_names, (
        "wheel is missing registry law templates (registry_data/laws) (#615)"
    )
    assert "symeraseme/registry_data/templates/report.html.j2" in wheel_names, (
        "wheel is missing registry templates (registry_data/templates) (#615)"
    )

    with tarfile.open(sdist) as tf:
        sdist_names = tf.getnames()
    sdist_broker_yamls = [
        n for n in sdist_names if "/registry/brokers/" in n and n.endswith(".yaml")
    ]
    assert sdist_broker_yamls, (
        "sdist is missing registry/brokers — Homebrew builds from the sdist and "
        "needs the tree present (#615)"
    )


def test_installed_wheel_brokers_list_returns_nonzero_count(
    tmp_path: Path,
    wheel_only_venv: Path,
) -> None:
    """``brokers list`` must find brokers in a wheel-only install (regression for #615)."""
    cli = wheel_only_venv / (
        "Scripts/symeraseme.exe" if sys.platform == "win32" else "bin/symeraseme"
    )

    # Never let the packaged process fall back to an env override or a host
    # checkout: the point is that the wheel itself carries the registry.
    env = {k: v for k, v in os.environ.items() if k != "SYMERASEME_RESOURCES"}
    env["HOME"] = str(tmp_path / "home")

    result = _run([str(cli), "--output", "json", "brokers", "list"], cwd=tmp_path, env=env)
    assert result.returncode == 0, (
        "brokers list failed in wheel-only venv:\n"
        f"  stdout: {result.stdout[:2000]}\n"
        f"  stderr: {result.stderr[:2000]}"
    )
    data = json.loads(result.stdout)
    assert data["count"] > 0, "brokers list returned zero brokers from the installed wheel (#615)"
