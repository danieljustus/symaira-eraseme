"""Adapter protocols for dependency injection."""

from __future__ import annotations

from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class WebFormRunner(Protocol):
    """Protocol for web-form automation adapters."""

    def __call__(
        self,
        broker_id: str,
        *,
        headed: bool = False,
        screenshot_dir: str = "",
        dry_run: bool = False,
    ) -> dict[str, Any]: ...
