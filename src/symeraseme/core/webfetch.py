"""Transparent HTTP fetch helper with symfetch integration.

When ``symfetch`` is installed on PATH, page fetches are delegated to
``symfetch get <URL> --format md`` for richer rendering (JS, CSS
extraction, markdown output).  Otherwise the standard-library
``urllib.request`` path is used as before.

Configuration
-------------
Set ``use_symfetch`` in the global config or pass ``use_symfetch``
explicitly to :func:`fetch_url`.

* ``true``  — prefer symfetch (default when installed)
* ``false`` — always use urllib fallback
"""

from __future__ import annotations

import logging
import shutil
import subprocess
from dataclasses import dataclass
from urllib.request import Request, urlopen

logger = logging.getLogger(__name__)

_SYMVAULT_TIMEOUT = 60  # seconds


@dataclass(frozen=True)
class FetchResult:
    url: str
    status_code: int
    body: str
    via: str  # "symfetch" | "urllib"


def symfetch_available() -> bool:
    """Return True if ``symfetch`` is on PATH."""
    return shutil.which("symfetch") is not None


def _fetch_via_symfetch(url: str, timeout: int = _SYMVAULT_TIMEOUT) -> FetchResult | None:
    """Fetch a URL using ``symfetch get <URL> --format md``."""
    if not symfetch_available():
        return None

    try:
        result = subprocess.run(
            ["symfetch", "get", url, "--format", "md"],
            capture_output=True,
            timeout=timeout,
        )
    except FileNotFoundError:
        return None
    except subprocess.TimeoutExpired:
        logger.warning("symfetch timed out after %ds for %s", timeout, url)
        return None
    except OSError as exc:
        logger.debug("symfetch execution failed: %s", exc)
        return None

    if result.returncode != 0:
        stderr_text = (result.stderr or b"").decode("utf-8", errors="replace").strip()
        logger.debug(
            "symfetch exited with code %d for %s: %s",
            result.returncode,
            url,
            stderr_text[:200],
        )
        return None

    body = result.stdout.decode("utf-8", errors="replace")
    return FetchResult(url=url, status_code=200, body=body, via="symfetch")


def _fetch_via_urllib(url: str, timeout: int = 30) -> FetchResult:
    """Fetch a URL using the standard library."""
    req = Request(url, headers={"User-Agent": "SymairaEraseMe/1.0"})
    with urlopen(req, timeout=timeout) as resp:
        body = resp.read().decode("utf-8", errors="replace")
        status = resp.status
    return FetchResult(url=url, status_code=status, body=body, via="urllib")


def fetch_url(
    url: str,
    *,
    use_symfetch: bool | None = None,
    timeout: int = 60,
) -> FetchResult:
    """Fetch a URL, preferring symfetch when available and enabled.

    Parameters
    ----------
    url:
        The URL to fetch.
    use_symfetch:
        * ``True``  — use symfetch (raises if not installed)
        * ``False`` — always use urllib
        * ``None``  — auto-detect (default)
    timeout:
        Maximum seconds to wait for the response.

    Returns
    -------
    FetchResult
        The fetched content with metadata.

    Raises
    ------
    RuntimeError
        If *use_symfetch* is ``True`` but symfetch is not installed.
    HTTPError | URLError
        If the urllib fallback fails.
    """
    if use_symfetch is True and not symfetch_available():
        raise RuntimeError("symfetch requested but not found on PATH")

    if use_symfetch is not False:
        result = _fetch_via_symfetch(url, timeout=timeout)
        if result is not None:
            return result

    return _fetch_via_urllib(url, timeout=timeout)
