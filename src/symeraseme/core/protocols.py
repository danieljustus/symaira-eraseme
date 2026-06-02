"""Adapter protocols for dependency injection."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class WebFormRunner(Protocol):
    def __call__(
        self,
        broker_id: str,
        *,
        headed: bool = False,
        screenshot_dir: str = "",
        dry_run: bool = False,
    ) -> dict[str, Any]: ...


@runtime_checkable
class EmailSender(Protocol):
    def __call__(
        self,
        to: str,
        subject: str,
        body: str,
        *,
        account: str | None = None,
        config_path: str | Path | None = None,
    ) -> dict[str, str]: ...
