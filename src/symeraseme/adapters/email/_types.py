"""Shared email types used by both Himalaya and SMTP/IMAP adapters."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime


@dataclass
class Envelope:
    id: str
    subject: str
    from_: str
    to: str
    date: datetime | None = None
    flags: list[str] = field(default_factory=list)


@dataclass
class Message:
    id: str
    subject: str
    from_: str
    to: str
    date: datetime | None = None
    body: str = ""
    flags: list[str] = field(default_factory=list)


@dataclass
class SmtpConfig:
    host: str = "localhost"
    port: int = 587
    username: str = ""
    password: str = ""
    use_tls: bool = True
    from_addr: str = ""
