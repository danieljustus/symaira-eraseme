"""Background inbox polling scheduler with push notifications.

Runs ``handle_poll_inbox`` on a configurable interval and dispatches
notifications to the host agent when new replies arrive.

Uses only the Python standard library (``threading``). No external
dependencies required.
"""

from __future__ import annotations

import logging
import signal
import threading
import time
from typing import Any

from symeraseme.core.db_connection import init_db
from symeraseme.core.notify import InboxNotification, get_notification_backend
from symeraseme.services.inbox import handle_poll_inbox

logger = logging.getLogger(__name__)

DEFAULT_INTERVAL_SECONDS = 15 * 60  # 15 minutes


class InboxWatcher:
    """Periodically polls the inbox and notifies the host agent on new mail.

    Parameters
    ----------
    interval_seconds:
        Seconds between poll cycles (default 900 = 15 min).
    poll_kwargs:
        Keyword arguments forwarded to :func:`handle_poll_inbox`
        (host, port, username, since_days, ssl, campaign_id, password).
    """

    def __init__(
        self,
        *,
        interval_seconds: int = DEFAULT_INTERVAL_SECONDS,
        poll_kwargs: dict[str, Any],
    ) -> None:
        if interval_seconds < 60:
            msg = "interval_seconds must be >= 60 (1 minute minimum)"
            raise ValueError(msg)

        self._interval = interval_seconds
        self._poll_kwargs = poll_kwargs
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None
        self._last_reply_count: int = 0

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def start(self) -> None:
        """Start the background polling thread."""
        if self._thread is not None and self._thread.is_alive():
            logger.warning("Watcher is already running")
            return

        init_db()
        self._last_reply_count = self._count_replies()
        self._stop_event.clear()

        self._thread = threading.Thread(
            target=self._run_loop,
            name="inbox-watcher",
            daemon=True,
        )
        self._thread.start()
        logger.info(
            "Inbox watcher started (interval=%ds, replies_known=%d)",
            self._interval,
            self._last_reply_count,
        )

    def stop(self) -> None:
        """Signal the watcher to stop and wait for the thread to finish."""
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=self._interval + 10)
            self._thread = None
        logger.info("Inbox watcher stopped")

    def poll_once(self) -> dict[str, Any]:
        """Run a single poll cycle and return the result dict."""
        return self._do_poll()

    @property
    def is_running(self) -> bool:
        return self._thread is not None and self._thread.is_alive()

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _run_loop(self) -> None:
        """Background loop that polls until stopped."""
        while not self._stop_event.is_set():
            try:
                self._do_poll()
            except Exception:
                logger.exception("Unhandled error during inbox poll cycle")
            self._stop_event.wait(timeout=self._interval)

    def _do_poll(self) -> dict[str, Any]:
        """Execute one poll cycle and send notifications for new replies."""
        result = handle_poll_inbox(**self._poll_kwargs)
        new_count = self._count_replies()
        new_replies = new_count - self._last_reply_count

        if new_replies > 0:
            logger.info("Detected %d new inbox reply(ies)", new_replies)
            self._send_notifications_for_new(new_count)

        self._last_reply_count = new_count

        data = result.data if isinstance(result.data, dict) else {}
        data["watcher_new_replies"] = max(new_replies, 0)
        return data

    def _count_replies(self) -> int:
        """Return total count of inbox_replies rows."""
        from symeraseme.core.db_connection import get_connection

        try:
            conn = get_connection()
            row = conn.execute("SELECT COUNT(*) FROM inbox_replies").fetchone()
            return row[0] if row else 0
        except Exception:
            logger.debug("Could not count inbox_replies", exc_info=True)
            return 0

    def _send_notifications_for_new(self, current_count: int) -> None:
        """Fetch the newest replies and send notifications."""
        from symeraseme.core.db_connection import get_connection

        try:
            conn = get_connection()
            rows = conn.execute(
                """SELECT ir.request_id, ir.from_addr, ir.subject, ir.snippet,
                          ir.received_at, rr.broker_id
                   FROM inbox_replies ir
                   LEFT JOIN removal_requests rr ON rr.id = ir.request_id
                   ORDER BY ir.id DESC
                   LIMIT ?""",
                (max(self._last_reply_count, 0) + 50,),
            ).fetchall()
        except Exception:
            logger.exception("Failed to query new replies for notification")
            return

        # Only notify for rows we haven't seen before
        seen = current_count - self._last_reply_count
        new_rows = rows[:seen] if seen > 0 else []

        backend = get_notification_backend()
        for row in new_rows:
            notification = InboxNotification(
                request_id=row["request_id"],
                broker_id=row["broker_id"] or "unknown",
                from_addr=row["from_addr"] or "",
                subject=row["subject"] or "(no subject)",
                snippet=row["snippet"] or "",
                received_at=row["received_at"] or "",
            )
            sent = backend.send(notification)
            if sent:
                logger.info("Notification sent: %s", notification.to_message())


def run_watch_loop(
    *,
    interval_seconds: int = DEFAULT_INTERVAL_SECONDS,
    poll_kwargs: dict[str, Any],
) -> None:
    """Run the watcher in the foreground with graceful SIGINT/SIGTERM handling.

    This blocks until the process receives a termination signal.
    """
    watcher = InboxWatcher(
        interval_seconds=interval_seconds,
        poll_kwargs=poll_kwargs,
    )

    def _shutdown(signum: int, _frame: Any) -> None:
        sig_name = signal.Signals(signum).name
        logger.info("Received %s — shutting down watcher…", sig_name)
        watcher.stop()

    prev_sigint = signal.signal(signal.SIGINT, _shutdown)
    prev_sigterm = signal.signal(signal.SIGTERM, _shutdown)

    try:
        watcher.start()
        # Block the main thread until stopped
        while watcher.is_running:
            time.sleep(1)
    finally:
        watcher.stop()
        signal.signal(signal.SIGINT, prev_sigint)
        signal.signal(signal.SIGTERM, prev_sigterm)
