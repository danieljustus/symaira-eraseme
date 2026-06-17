"""Symaira EraseMe — Automated data broker removal tool."""

from __future__ import annotations

import tomllib
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path

try:
    __version__ = version("symeraseme")
except PackageNotFoundError:
    # Package not installed (e.g. running from source without pip install).
    # Read the version from pyproject.toml so it never drifts.
    try:
        _pyproject = Path(__file__).resolve().parent.parent.parent / "pyproject.toml"
        with open(_pyproject, "rb") as _f:
            __version__ = tomllib.load(_f)["project"]["version"]
    except (FileNotFoundError, KeyError):
        __version__ = "0.0.0"
