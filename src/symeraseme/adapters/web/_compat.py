"""Playwright compatibility shim for optional [web] extra.

Provides a narrow exception type that can be caught without importing
playwright directly, so modules that do not depend on the optional
``[web]`` extra can still handle Playwright errors precisely.
"""

try:
    from playwright._impl._errors import Error as _PlaywrightError
except Exception:  # pragma: no cover
    _PlaywrightError = RuntimeError  # type: ignore[misc,assignment]


class PlaywrightError(_PlaywrightError):  # type: ignore[misc,valid-type]
    """Raised when a Playwright operation fails.

    When the ``[web]`` extra is installed this inherits from
    ``playwright._impl._errors.Error`` so existing ``except Error``
    handlers in Playwright-centric code continue to work.

    When Playwright is *not* installed it falls back to ``RuntimeError``
    so callers can still catch it without an unconditional import of the
    optional dependency.
    """
