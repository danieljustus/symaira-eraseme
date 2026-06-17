"""Unified secret resolution: vault:// URIs → env vars → keyring.

All credential lookups in Symaira EraseMe should funnel through
:func:`resolve_secret` so that ``vault://`` references are transparently
resolved via the ``symvault`` CLI while plain values fall back to
environment variables or the system keyring.

Security contract
-----------------
* The resolved secret value is **never** written to structured logs
  (structlog), Python ``logging`` output, or tracebacks.
* ``symvault`` is invoked as a subprocess with ``capture_output=True``
  and a 5-second timeout to prevent hangs.
* If ``symvault`` is not installed, a non-zero exit code is returned, or
  the timeout expires, the function silently falls through to the next
  resolution layer.
"""

from __future__ import annotations

import logging
import os
import shutil
import subprocess

logger = logging.getLogger(__name__)

_VAULT_PREFIX = "vault://"
_SYMVAULT_TIMEOUT = 5  # seconds


class SecretResolutionError(Exception):
    """Raised when all resolution layers fail for a vault:// URI."""


def _symvault_available() -> bool:
    """Return True if ``symvault`` is on PATH."""
    return shutil.which("symvault") is not None


def _resolve_via_symvault(path: str) -> str | None:
    """Call ``symvault get <path> --print`` and return stdout, or None on failure.

    The secret value is never logged.
    """
    if not _symvault_available():
        logger.debug("symvault not found on PATH; skipping vault resolution")
        return None

    try:
        result = subprocess.run(
            ["symvault", "get", path, "--print"],
            capture_output=True,
            timeout=_SYMVAULT_TIMEOUT,
        )
    except FileNotFoundError:
        # shutil.which lied or PATH changed between check and run.
        logger.debug("symvault disappeared from PATH between check and run")
        return None
    except subprocess.TimeoutExpired:
        logger.warning("symvault get %s timed out after %ds", path, _SYMVAULT_TIMEOUT)
        return None
    except OSError as exc:
        logger.warning("Failed to execute symvault: %s", exc)
        return None

    if result.returncode != 0:
        logger.warning(
            "symvault get failed with exit code %d (path omitted for security)",
            result.returncode,
        )
        return None

    secret = result.stdout.decode("utf-8", errors="replace").strip()
    if not secret:
        logger.warning("symvault get %s returned empty output", path)
        return None

    return secret


def _resolve_via_env(env_var: str) -> str | None:
    """Read a plain-text value from an environment variable."""
    value = os.environ.get(env_var, "")
    return value if value else None


def _resolve_via_keyring(service: str, username: str) -> str | None:
    """Read a plain-text value from the system keyring."""
    try:
        import keyring as _keyring

        value = _keyring.get_password(service, username)
        return value if value else None
    except ImportError:
        logger.debug("keyring package not installed; skipping keyring resolution")
        return None
    except (OSError, _keyring.errors.KeyringError) as exc:
        logger.debug("Keyring resolution failed (%s: %s); skipping", type(exc).__name__, exc)
        return None


def resolve_secret(
    value: str,
    *,
    env_fallback: str | None = None,
    keyring_service: str | None = None,
) -> str:
    """Resolve a secret value through the fallback chain.

    Parameters
    ----------
    value:
        The raw value to resolve.  If it starts with ``vault://``, the
        path portion is passed to ``symvault get <path> --print``.  Otherwise the
        literal value is returned immediately.
    env_fallback:
        Environment variable name to check when vault:// resolution
        fails or is skipped.  For example ``"ANTHROPIC_API_KEY"``.
    keyring_service:
        Keyring service name (with ``keyring_username`` defaulting to
        ``"symeraseme"``).  If provided, the keyring is queried after
        the env var fallback fails.

    Returns
    -------
    str
        The resolved secret.

    Raises
    ------
    SecretResolutionError
        If no layer could produce a value.
    """
    # --- Layer 0: literal value (not a vault:// URI) ---
    if not value.startswith(_VAULT_PREFIX):
        return value

    vault_path = value[len(_VAULT_PREFIX) :]
    if not vault_path:
        raise SecretResolutionError(
            "vault:// URI is empty; provide a path like vault://github/api-token"
        )

    # --- Layer 1: symvault ---
    secret = _resolve_via_symvault(vault_path)
    if secret is not None:
        return secret

    # --- Layer 2: environment variable ---
    if env_fallback:
        secret = _resolve_via_env(env_fallback)
        if secret is not None:
            logger.debug(
                "Resolved secret from env var %s (vault://%s unavailable)",
                env_fallback,
                vault_path,
            )
            return secret

    # --- Layer 3: system keyring ---
    if keyring_service:
        secret = _resolve_via_keyring(keyring_service, vault_path)
        if secret is not None:
            logger.debug(
                "Resolved secret from keyring %s (vault://%s unavailable)",
                keyring_service,
                vault_path,
            )
            return secret

    # --- All layers exhausted ---
    msg = f"Cannot resolve secret 'vault://{vault_path}': symvault not available or returned error"
    if env_fallback:
        msg += f", env var '{env_fallback}' not set"
    if keyring_service:
        msg += f", keyring '{keyring_service}' has no entry for '{vault_path}'"
    msg += ". Set the value directly or install symvault."
    raise SecretResolutionError(msg)
