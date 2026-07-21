"""SMTP configuration loading from environment variables."""

from __future__ import annotations

import os

from symeraseme.adapters.email._types import SmtpConfig


def load_smtp_config() -> SmtpConfig:
    """Load SMTP configuration from environment variables.

    Reads:
        SYMERASEME_SMTP_HOST       (default: localhost)
        SYMERASEME_SMTP_PORT       (default: 587)
        SYMERASEME_SMTP_USER       (default: "")
        SYMERASEME_SMTP_PASSWORD   (default: "")
        SYMERASEME_SMTP_TLS        (default: 1)
        SYMERASEME_SMTP_FROM       (default: "")
    """
    return SmtpConfig(
        host=os.environ.get("SYMERASEME_SMTP_HOST", "localhost"),
        port=int(os.environ.get("SYMERASEME_SMTP_PORT", "587")),
        username=os.environ.get("SYMERASEME_SMTP_USER", ""),
        password=os.environ.get("SYMERASEME_SMTP_PASSWORD", ""),
        use_tls=os.environ.get("SYMERASEME_SMTP_TLS", "1").lower() in ("1", "true", "yes"),
        from_addr=os.environ.get("SYMERASEME_SMTP_FROM", ""),
    )
