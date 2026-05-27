from __future__ import annotations

import logging
import os
import re
from collections.abc import Callable
from pathlib import Path

logger = logging.getLogger(__name__)

_LLM_CONSENT_FILE = Path("~/.config/symeraseme/.llm_consent_granted").expanduser()

_EMAIL_PATTERN = re.compile(
    r"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9]"
    r"(?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?"
    r"(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*"
)

_PHONE_PATTERN = re.compile(r"(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}")

_SSN_PATTERN = re.compile(r"\b(?!000|666|9\d{2})\d{3}[- ]?(?!00)\d{2}[- ]?(?!0000)\d{4}\b")


def _scrub_email(match: re.Match) -> str:
    local, domain = match.group(0).split("@", 1)
    if len(local) <= 2:
        visible = local[0] if local else "*"
    else:
        visible = local[:1] + "*" * (len(local) - 2) + local[-1:]
    domain_parts = domain.split(".")
    if len(domain_parts) >= 2:
        domain_display = domain_parts[0][:1] + "*." + ".".join(domain_parts[1:])
    else:
        domain_display = domain_parts[0][:1] + ".*"
    return f"{visible}@{domain_display}"


def _scrub_phone(match: re.Match) -> str:
    digits = re.sub(r"\D", "", match.group(0))
    if len(digits) == 11:
        return f"+1-***-***-{digits[-4:]}"
    return f"***-***-{digits[-4:]}"


def _scrub_ssn(match: re.Match) -> str:
    return "***-**-****"


_SCRUBBERS: list[tuple[re.Pattern, Callable]] = [
    (_EMAIL_PATTERN, _scrub_email),
    (_PHONE_PATTERN, _scrub_phone),
    (_SSN_PATTERN, _scrub_ssn),
]


def scrub_pii(text: str) -> str:
    for pattern, replacer in _SCRUBBERS:
        text = pattern.sub(replacer, text)
    return text


def llm_consent_granted() -> bool:
    if _LLM_CONSENT_FILE.exists():
        return True
    env = os.environ.get("SYMERASEME_LLM_CONSENT", "").strip().lower()
    return env in ("1", "true", "yes")


def grant_llm_consent() -> None:
    _LLM_CONSENT_FILE.parent.mkdir(parents=True, exist_ok=True)
    _LLM_CONSENT_FILE.touch()
    logger.info("LLM PII consent granted via %s", _LLM_CONSENT_FILE)
