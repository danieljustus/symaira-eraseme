"""LLM-powered rebuttal generator for broker rejection replies.

When brokers reject or challenge removal requests, this module selects the
appropriate rebuttal template based on LLM classification of the rejection
reason, falling back to the templating engine when the LLM is unavailable.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from symeraseme.adapters.triage.scrubber import redact_profile_pii
from symeraseme.llm.protocol import LLMClient, LLMClientError
from symeraseme.registry.schema import IdentityProfile

logger = logging.getLogger(__name__)

# Mapping from rejection type to template name and human-readable label
REJECTION_TEMPLATES: dict[str, dict[str, str]] = {
    "address_mismatch": {
        "template": "gdpr-rebuttal-address.md.j2",
        "label": "Address Discrepancy Rebuttal (GDPR)",
        "description": "Broker rejected request citing old address on file",
        "jurisdiction": "GDPR",
    },
    "identity_challenged": {
        "template": "gdpr-rebuttal-identity.md.j2",
        "label": "Identity Verification Rebuttal (GDPR)",
        "description": "Broker challenged identity verification",
        "jurisdiction": "GDPR",
    },
    "ccpa_identity_challenged": {
        "template": "ccpa-rebuttal-deletion.md.j2",
        "label": "Identity Verification Rebuttal (CCPA)",
        "description": "Broker requested additional identity info under CCPA",
        "jurisdiction": "CCPA",
    },
}

# Fallback templates used when LLM classification is unavailable
FALLBACK_KEYWORDS: list[tuple[list[str], str]] = [
    (["address", "old address", "previous address", "current address"], "address_mismatch"),
    (["identity", "verify", "verification", "ID", "passport", "driver"], "identity_challenged"),
    (["CCPA", "california", "Section 1798"], "ccpa_identity_challenged"),
]

REBUTTAL_SYSTEM_PROMPT = (
    "You are a precise rejection classifier for a data broker removal tool.\n\n"
    "Your task is to analyze a broker's response to a data erasure request and\n"
    "determine the correct rebuttal strategy. Read the broker's message and\n"
    "classify the rejection reason into one of these categories:\n\n"
    "- **address_mismatch**: Broker claims the address on file does not match.\n"
    "- **identity_challenged**: Broker requests additional identity verification (GDPR).\n"
    "- **ccpa_identity_challenged**: Broker requests additional identity verification (CCPA).\n"
    "- **other**: Rejection does not fit any known category — user needs to review.\n\n"
    "Respond with ONLY a JSON object on a single line:\n"
    '{"classification": "<label>", "confidence": 0.0-1.0, "summary": "<text>", '
    '"key_points": ["..."], "jurisdiction": "<GDPR|CCPA|unknown>"}\n\n'
    "Examples:\n"
    "- Address mismatch: address on file does not match\n"
    '  → {"classification": "address_mismatch", "confidence": 0.95, '
    '"summary": "Address mismatch", "key_points": ["Address does not match"], '
    '"jurisdiction": "GDPR"}\n\n'
    "- Identity verification: passport or driving license required\n"
    '  → {"classification": "identity_challenged", "confidence": 0.9, '
    '"summary": "ID verification required", "key_points": ["Passport required"], '
    '"jurisdiction": "GDPR"}'
)


@dataclass
class RebuttalResult:
    template_name: str
    label: str
    description: str
    jurisdiction: str
    rejection_classification: str
    confidence: float
    rebuttal_body: str
    needs_human_review: bool = False
    llm_used: bool = False
    usage_record: Any = None


@dataclass
class RejectionClassification:
    classification: str
    confidence: float
    summary: str
    key_points: list[str] = field(default_factory=list)
    jurisdiction: str = "unknown"


def _select_fallback_template(broker_message: str) -> str | None:
    """Select a rebuttal template based on keyword matching."""
    message_lower = broker_message.lower()
    for keywords, template_key in FALLBACK_KEYWORDS:
        for kw in keywords:
            if kw.lower() in message_lower:
                return template_key
    return None


def _build_classifier_user_prompt(
    *,
    broker_name: str,
    broker_message: str,
    original_request_template: str = "",
) -> str:
    """Build the user prompt for the rejection classifier."""
    parts = [f"Broker: {broker_name or 'Unknown'}"]
    if original_request_template:
        parts.append(f"Original request (truncated):\n{original_request_template[:500]}")
    parts.append(f"\nBroker response:\n{redact_profile_pii(broker_message[:3000])}")
    return "\n\n".join(parts)


def _parse_classification_response(response_text: str) -> RejectionClassification:
    """Parse the JSON response from the LLM into a RejectionClassification."""
    import json

    text = response_text.strip()
    if text.startswith("```"):
        text = text.strip("`").strip()
        if text.startswith("json"):
            text = text[4:].strip()

    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        logger.warning("Failed to parse classifier JSON: %s", text[:200])
        return RejectionClassification(
            classification="other",
            confidence=0.0,
            summary="Failed to parse classifier response",
        )

    classification = str(data.get("classification", "other")).lower().strip()
    if classification not in REJECTION_TEMPLATES and classification != "other":
        classification = "other"

    confidence = float(data.get("confidence", 0.0))
    confidence = max(0.0, min(1.0, confidence))

    summary = str(data.get("summary", ""))[:200]
    key_points = data.get("key_points", [])
    if not isinstance(key_points, list):
        key_points = []
    jurisdiction = str(data.get("jurisdiction", "unknown"))

    return RejectionClassification(
        classification=classification,
        confidence=confidence,
        summary=summary,
        key_points=key_points,
        jurisdiction=jurisdiction,
    )


def generate_rebuttal(
    *,
    broker_name: str = "",
    broker_website: str = "",
    broker_message: str = "",
    original_request_template: str = "",
    original_request_date: str = "",
    profile: IdentityProfile | None = None,
    client: LLMClient | None = None,
    templates_dir: str | Path | None = None,
) -> RebuttalResult:
    """Generate a rebuttal email for a broker rejection.

    Uses LLM to classify the rejection reason and select the appropriate
    template. Falls back to keyword matching when the LLM is unavailable.
    """
    from symeraseme.core.templating import list_templates, render_template

    # Step 1: Classify the rejection using LLM (or fallback)
    rejection_classification: RejectionClassification | None = None
    llm_used = False

    # Try LLM classification first
    if client is None:
        from symeraseme.llm.factory import create_llm_client

        client = create_llm_client()

    if client is not None and client.is_available():
        try:
            user_prompt = _build_classifier_user_prompt(
                broker_name=broker_name,
                broker_message=broker_message,
                original_request_template=original_request_template,
            )
            response_text, usage = client.classify(
                system_prompt=REBUTTAL_SYSTEM_PROMPT,
                user_prompt=user_prompt,
                cache_key=f"rebuttal:{broker_name}",
            )
            rejection_classification = _parse_classification_response(response_text)
            rejection_classification.key_points = getattr(
                rejection_classification, "key_points", []
            )
            llm_used = True
            usage_record = usage
        except LLMClientError as e:
            logger.warning("LLM classification failed: %s — using fallback", e)
            rejection_classification = None
            usage_record = None
    else:
        logger.info("LLM API not available, using fallback classification")
        usage_record = None

    # Step 2: Determine template key
    template_key: str | None = None
    if rejection_classification and rejection_classification.classification in REJECTION_TEMPLATES:
        template_key = rejection_classification.classification
    else:
        # Fallback: keyword matching
        template_key = _select_fallback_template(broker_message)

    if template_key is None:
        template_key = "identity_challenged"  # safest default

    template_info = REJECTION_TEMPLATES.get(
        template_key, REJECTION_TEMPLATES["identity_challenged"]
    )
    template_name = template_info["template"]

    # Step 3: Verify template exists
    available = list_templates(templates_dir)
    if template_name not in available:
        # Find the best match
        jurisdiction = template_info["jurisdiction"]
        fallback_candidates = [t for t in available if t.startswith(jurisdiction.lower())]
        if fallback_candidates:
            template_name = fallback_candidates[0]
        else:
            template_name = "gdpr-art17.en.md.j2"  # ultimate fallback
            template_info = {
                "label": "GDPR Erasure Request (Fallback)",
                "description": "Fallback template when rebuttal template not found",
                "jurisdiction": "GDPR",
            }

    # Step 4: Render the template
    rebuttal_body = render_template(
        template_name,
        profile=profile,
        broker_name=broker_name,
        broker_website=broker_website,
        templates_dir=templates_dir,
        extra_vars={
            "original_request_date": original_request_date,
        },
    )

    needs_review = (
        rejection_classification is None
        or rejection_classification.classification == "other"
        or (rejection_classification is not None and rejection_classification.confidence < 0.5)
    )

    return RebuttalResult(
        template_name=template_name,
        label=template_info.get("label", "Rebuttal"),
        description=template_info.get("description", ""),
        jurisdiction=template_info.get("jurisdiction", "unknown"),
        rejection_classification=(
            rejection_classification.classification if rejection_classification else "fallback"
        ),
        confidence=rejection_classification.confidence if rejection_classification else 0.0,
        rebuttal_body=rebuttal_body,
        needs_human_review=needs_review,
        llm_used=llm_used,
        usage_record=usage_record,
    )
