"""Compatibility helpers for platform-specific issues.

This module provides helper functions to detect and handle platform-specific
issues, such as the pydantic_core LINKEDIT alignment issue on macOS 27 (Tahoe).
"""

from __future__ import annotations

import platform
import sys


def _detect_macos_version() -> tuple[int, ...] | None:
    """Detect the macOS version as a tuple of (major, minor, patch).

    Returns None if not running on macOS.
    """
    if sys.platform != "darwin":
        return None
    try:
        version_str = platform.mac_ver()[0]
        if not version_str:
            return None
        parts = version_str.split(".")
        return tuple(int(p) for p in parts[:3])
    except (AttributeError, ValueError):
        return None


def check_pydantic_core_compat() -> None:
    """Check for pydantic_core compatibility issues and provide helpful messages.

    On macOS 27 (Tahoe) and later, the pre-built pydantic_core wheels may have
    LINKEDIT alignment issues that cause ImportError. This function detects
    the issue and provides a helpful error message with instructions.

    Raises:
        ImportError: If pydantic_core is incompatible with the current platform.
    """
    try:
        from pydantic_core import __version__  # noqa: F401
    except ImportError as exc:
        error_msg = str(exc)
        if "mis-aligned LINKEDIT" in error_msg or "LINKEDIT" in error_msg:
            macos_version = _detect_macos_version()
            if macos_version and macos_version >= (27, 0, 0):
                major, minor = macos_version[0], macos_version[1]
                raise ImportError(
                    f"pydantic_core is incompatible with macOS {major}.{minor} "
                    f"(Tahoe). The pre-built wheel has LINKEDIT alignment issues.\n\n"
                    "To fix this, reinstall with pydantic_core built from source:\n\n"
                    "  pip install --force-reinstall --no-binary pydantic_core pydantic-core\n\n"
                    "Or upgrade to a newer version of pydantic that includes a compatible "
                    "pydantic_core wheel:\n\n"
                    "  pip install --upgrade pydantic\n\n"
                    "For more information, see: https://github.com/danieljustus/symaira-eraseme/issues?q=pydantic+LINKEDIT"
                ) from exc
        raise
