"""Symaira EraseMe — Automated data broker removal tool."""

from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("symeraseme")
except PackageNotFoundError:
    # Package not installed (e.g. running from source without pip install)
    __version__ = "0.3.0"
