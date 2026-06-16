from __future__ import annotations

import getpass
import json
import logging
import os
import re
import time
from collections.abc import Callable
from pathlib import Path

from symeraseme.core.config import get_config

logger = logging.getLogger(__name__)


def _llm_consent_file() -> Path:
    return get_config().resolved_config_dir / ".llm_consent_granted"

# Bounded quantifiers prevent catastrophic backtracking on pathological
# input (long strings of dots/hyphens).  126 labels is far beyond any
# real domain while keeping the match linear in input length.
_EMAIL_PATTERN = re.compile(
    r"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]{1,64}@[a-zA-Z0-9]"
    r"(?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?"
    r"(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?){0,126}"
)

_PHONE_PATTERN = re.compile(r"(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}")

_SSN_PATTERN = re.compile(r"\b(?!000|666|9\d{2})\d{3}[- ]?(?!00)\d{2}[- ]?(?!0000)\d{4}\b")

# EU PII patterns — extend beyond US-centric coverage for GDPR-focused tooling.
# IBAN: ISO 13616 format (country code + check digits + up to 30 alphanumeric chars).
_IBAN_PATTERN = re.compile(r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b")

# German Personalausweis (national ID): one letter (A–L), 8 digits, optional letter.
_DE_ID_PATTERN = re.compile(r"\b[A-L]\d{8}[A-Z]?\b")

# French NIR (numéro de sécurité sociale): 13 digits with embedded birth metadata.
_FR_ID_PATTERN = re.compile(r"\b[12]\d{2}(0[1-9]|1[0-2])\d{5}\d{3}(\d{2})?\b")

# Spanish DNI: 8 digits followed by a checksum letter (I, Ñ, O, U excluded).
_ES_ID_PATTERN = re.compile(r"\b\d{8}[A-HJ-NP-TV-Z]\b")

# Passport numbers: 6–9 uppercase alphanumeric chars with word boundaries.
# Preceded by a hint keyword to reduce false positives in free text.
_PASSPORT_PATTERN = re.compile(
    r"(?:passport|travel\s*document|reisedokument)\s*(?:#|no|num|number)?\s*[:.]?\s*"
    r"([A-Z0-9]{6,9})\b",
    re.IGNORECASE,
)


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


def _scrub_iban(match: re.Match) -> str:
    raw = match.group(0)
    return raw[:2] + "**" + "*" * max(0, len(raw) - 4) + raw[-4:]


def _scrub_de_id(match: re.Match) -> str:
    return "*******" + match.group(0)[-2:]


def _scrub_fr_id(match: re.Match) -> str:
    return "***" + match.group(0)[-3:]


def _scrub_es_id(match: re.Match) -> str:
    return "****-****-" + match.group(0)[-1]


def _scrub_passport(match: re.Match) -> str:
    pn = match.group(1)
    return match.group(0).replace(pn, "*" * max(3, len(pn) - 2) + pn[-2:])


# Pre-filter: fast early exit when text contains no PII-like characters.
# All 8 PII patterns below require at least one digit or an '@' character
# to produce a match, so a single scan avoids running 8 regex substitutions
# on PII-free input.
_PII_TRIGGER_RE = re.compile(r"@|\d")

_SCRUBBERS: list[tuple[re.Pattern, Callable]] = [
    (_IBAN_PATTERN, _scrub_iban),
    (_DE_ID_PATTERN, _scrub_de_id),
    (_FR_ID_PATTERN, _scrub_fr_id),
    (_ES_ID_PATTERN, _scrub_es_id),
    (_PASSPORT_PATTERN, _scrub_passport),
    (_SSN_PATTERN, _scrub_ssn),
    (_EMAIL_PATTERN, _scrub_email),
    (_PHONE_PATTERN, _scrub_phone),
]


def scrub_pii(text: str) -> str:
    if not _PII_TRIGGER_RE.search(text):
        return text
    for pattern, replacer in _SCRUBBERS:
        text = pattern.sub(replacer, text)
    return text


def llm_consent_granted() -> bool:
    env = os.environ.get("SYMERASEME_LLM_CONSENT", "").strip().lower()
    if env in ("1", "true", "yes"):
        return True
    if not _llm_consent_file().exists():
        return False
    # Legacy empty touch file (st_size == 0) — treat as granted for backward compatibility
    if _llm_consent_file().stat().st_size == 0:
        return True
    try:
        data = json.loads(_llm_consent_file().read_text(encoding="utf-8"))
        return bool(data.get("granted", False))
    except (json.JSONDecodeError, OSError) as exc:
        logger.warning(
            "Cannot read LLM consent file %s (%s) — denying consent; re-run 'grant llm-consent'",
            _llm_consent_file(),
            exc,
        )
        return False


def grant_llm_consent() -> None:
    _llm_consent_file().parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    record = {
        "granted": True,
        "user": getpass.getuser(),
        "granted_at": int(time.time()),
        "scope": "llm_pii",
    }
    fd = os.open(_llm_consent_file(), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode=0o600)
    with open(fd, "w", encoding="utf-8") as f:
        json.dump(record, f, indent=2)
    logger.info("LLM PII consent granted via %s", _llm_consent_file())


def revoke_llm_consent() -> None:
    if _llm_consent_file().exists():
        _llm_consent_file().unlink(missing_ok=True)
        logger.info("LLM PII consent revoked (%s removed)", _llm_consent_file())
