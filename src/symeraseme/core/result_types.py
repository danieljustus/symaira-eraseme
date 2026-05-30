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
    data: dict[str, Any] = field(default_factory=dict)
    error: str | None = None

    @property
    def message(self) -> str:
        """Human-readable summary string."""
        return self.data.get("message", "") or self.error or ""

    def to_json(self) -> str:
        """Serialize to JSON string."""
        import json

        return json.dumps(
            {
                "success": self.success,
                "data": self.data,
                "error": self.error,
            },
            indent=2,
            default=str,
        )
