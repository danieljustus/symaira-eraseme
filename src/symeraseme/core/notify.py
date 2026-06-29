"""Notification system for host agent push notifications.

When ``poll-inbox --watch`` detects new broker replies, this module dispatches
notifications through configurable backends (file, webhook, or composite).

No external dependencies — uses only the Python standard library.
"""

from __future__ import annotations

import json
import logging
import os
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Protocol

from symeraseme.core.config import get_config

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Notification payload
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class InboxNotification:
    """Immutable notification about a new inbox reply."""

    request_id: int | None
    broker_id: str
    from_addr: str
    subject: str
    snippet: str
    received_at: str

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": "inbox_reply",
            "request_id": self.request_id,
            "broker_id": self.broker_id,
            "from_addr": self.from_addr,
            "subject": self.subject,
            "snippet": self.snippet,
            "received_at": self.received_at,
            "notified_at": datetime.now(UTC).isoformat(),
        }

    def to_message(self) -> str:
        rid = f"#{self.request_id}" if self.request_id else "unmatched"
        return f"New reply from {self.broker_id} received (request {rid}). Should I classify it?"


# ---------------------------------------------------------------------------
# Backend protocol
# ---------------------------------------------------------------------------


class NotificationBackend(Protocol):
    """Interface for notification delivery backends."""

    def send(self, notification: InboxNotification) -> bool: ...


# ---------------------------------------------------------------------------
# File backend — writes JSON events for host-agent polling
# ---------------------------------------------------------------------------


class FileNotificationBackend:
    """Write notification events as JSON files for host-agent polling."""

    def __init__(self, events_dir: Path | None = None) -> None:
        self.events_dir = events_dir or get_config().resolved_data_dir / "events"
        self.events_dir.mkdir(parents=True, exist_ok=True)

    def send(self, notification: InboxNotification) -> bool:
        try:
            ts = datetime.now(UTC).strftime("%Y%m%dT%H%M%S")
            rid = notification.request_id or "unknown"
            filename = f"inbox_reply_{ts}_{rid}.json"
            filepath = self.events_dir / filename
            filepath.write_text(json.dumps(notification.to_dict(), indent=2) + "\n")
            logger.info("Notification written to %s", filepath)
            return True
        except OSError as exc:
            logger.error("Failed to write notification file: %s", exc)
            return False


# ---------------------------------------------------------------------------
# Webhook backend — POST JSON to a URL
# ---------------------------------------------------------------------------


class WebhookNotificationBackend:
    """POST notification payloads to a webhook URL."""

    def __init__(self, webhook_url: str, timeout: int = 10) -> None:
        self.webhook_url = webhook_url
        self.timeout = timeout

    def send(self, notification: InboxNotification) -> bool:
        try:
            data = json.dumps(notification.to_dict()).encode()
            req = urllib.request.Request(
                self.webhook_url,
                data=data,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                ok = resp.status < 400
                if not ok:
                    logger.warning("Webhook returned status %d", resp.status)
                return ok
        except (urllib.error.URLError, OSError) as exc:
            logger.error("Webhook notification failed: %s", exc)
            return False


# ---------------------------------------------------------------------------
# Composite backend — fan-out to multiple backends
# ---------------------------------------------------------------------------


class CompositeNotificationBackend:
    """Dispatch notifications to all registered backends."""

    def __init__(self, backends: list[NotificationBackend]) -> None:
        self._backends = backends

    def send(self, notification: InboxNotification) -> bool:
        results = [b.send(notification) for b in self._backends]
        return any(results)


# ---------------------------------------------------------------------------
# Factory
# ---------------------------------------------------------------------------


def get_notification_backend() -> NotificationBackend:
    """Build the notification backend from environment configuration.

    Environment variables:
        SYMERASEME_EVENTS_DIR  — directory for file notifications
        SYMERASEME_WEBHOOK_URL — webhook URL for POST notifications

    At minimum the file backend is always active.
    """
    backends: list[NotificationBackend] = []

    events_dir_str = os.environ.get("SYMERASEME_EVENTS_DIR")
    events_dir = Path(events_dir_str).expanduser() if events_dir_str else None
    backends.append(FileNotificationBackend(events_dir))

    webhook_url = os.environ.get("SYMERASEME_WEBHOOK_URL")
    if webhook_url:
        backends.append(WebhookNotificationBackend(webhook_url))

    if len(backends) == 1:
        return backends[0]
    return CompositeNotificationBackend(backends)
