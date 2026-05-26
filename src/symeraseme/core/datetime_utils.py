"""Shared datetime parsing utilities."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

ISO_FORMATS = (
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M:%S%z",
    "%Y-%m-%d %H:%M:%S",
)

EMAIL_FORMATS = (
    "%Y-%m-%d %H:%M:%S %z",
    "%a, %d %b %Y %H:%M:%S %z",
)

ALL_FORMATS = ISO_FORMATS + EMAIL_FORMATS


def parse_iso_datetime(value: Any) -> datetime | None:
    """Parse a datetime string into a timezone-aware datetime object.

    Supports ISO 8601 formats and RFC 2822 email date formats.
    Returns None for None, empty strings, or unparseable values.
    Already-aware datetime objects are returned as-is.
    """
    if value is None:
        return None
    if isinstance(value, datetime):
        return value
    if isinstance(value, str):
        if not value:
            return None
        for fmt in ALL_FORMATS:
            try:
                parsed = datetime.strptime(value, fmt)
                if fmt in ISO_FORMATS:
                    parsed = parsed.replace(tzinfo=UTC)
                return parsed
            except ValueError:
                continue
        stripped = value.rstrip("Z")
        if stripped != value:
            for fmt in ISO_FORMATS:
                try:
                    return datetime.strptime(stripped, fmt).replace(tzinfo=UTC)
                except ValueError:
                    continue
    return None
