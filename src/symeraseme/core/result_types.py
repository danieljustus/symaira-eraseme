"""Structured result types shared across CLI and service layers."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class CliResult:
    """Structured result from a CLI command handler.

    Each handler returns a ``CliResult`` instead of a raw string, so the
    CLI layer can render it either as human-readable text (via rich) or as
    structured JSON.
    """

    success: bool = True
    data: dict[str, Any] | list[Any] = field(default_factory=dict)
    error: str | None = None

    def __init__(
        self,
        success: bool = True,
        data: dict[str, Any] | list[Any] | None = None,
        error: str | None = None,
        message: str = "",
    ) -> None:
        self.success = success
        self.data = data if data is not None else {}
        self.error = error
        self._message = message

    @property
    def message(self) -> str:
        """Human-readable summary string."""
        if isinstance(self.data, dict):
            return self._message or self.data.get("message", "") or self.error or ""
        return self._message or self.error or ""

    def to_json(self) -> str:
        """Serialize result to JSON string."""
        import json

        payload: dict[str, Any] = {"success": self.success}
        if self.error:
            payload["error"] = self.error
        else:
            payload["message"] = self.message
        if isinstance(self.data, dict):
            payload.update(self.data)
        elif self.data:
            payload["data"] = self.data
        return json.dumps(payload, indent=2, default=str)
