"""Structured result types shared across CLI and service layers."""

from __future__ import annotations

import dataclasses
import datetime
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


def _json_default(obj: Any) -> Any:
    if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
        return dataclasses.asdict(obj)
    if hasattr(obj, "model_dump"):
        return obj.model_dump(mode="json")
    if hasattr(obj, "dict"):
        return obj.dict()
    if isinstance(obj, (datetime.date, datetime.datetime)):
        return obj.isoformat()
    if isinstance(obj, Path):
        return str(obj)
    raise TypeError(f"Object of type {type(obj).__name__} is not JSON serializable")


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
        payload: dict[str, Any] = {"success": self.success}
        if self.error:
            payload["error"] = self.error
        else:
            payload["message"] = self.message
        if isinstance(self.data, dict):
            data_to_spread = self.data
            if self.error and "message" in data_to_spread:
                data_to_spread = {k: v for k, v in data_to_spread.items() if k != "message"}
            payload.update(data_to_spread)
        elif self.data:
            payload["data"] = self.data
        return json.dumps(payload, indent=2, default=_json_default)
