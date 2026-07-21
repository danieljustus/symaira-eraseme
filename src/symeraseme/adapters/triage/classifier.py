"""LLM-powered classifier for broker reply triage.

Maps incoming broker replies to structured event types using
a generic LLM client.
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from typing import Any

from symeraseme.adapters.triage.scrubber import redact_profile_pii
from symeraseme.llm.protocol import LLMClient, LLMClientError

logger = logging.getLogger(__name__)

CLASSIFICATION_LABELS = frozenset(
    {
        "ack",
        "confirmed",
        "rejected",
        "verification",
        "human_required",
        "autoresponder",
        "bounce",
        "unclear",
    }
)

CLASSIFICATION_TO_EVENT = {
    "ack": "ACK",
    "confirmed": "CONFIRMED",
    "rejected": "REJECTED_FINAL",
    "verification": "VERIFICATION_REQUESTED",
    "human_required": "HUMAN_ACTION_REQUIRED",
    "autoresponder": "AUTORESPONDER",
    "bounce": "BOUNCE",
    "unclear": "HUMAN_ACTION_REQUIRED",
}

CONFIDENCE_THRESHOLD_LOW = 0.4
CONFIDENCE_THRESHOLD_HIGH = 0.85

SYSTEM_PROMPT = """You are a precise email classifier for a data broker removal tool.

Your task is to classify incoming broker reply emails into one of these categories:

- **ack**: Broker acknowledges receipt. They are processing the request.
- **confirmed**: Broker confirms the data has been deleted or account closed.
- **rejected**: Broker explicitly rejects the request (invalid ID, not their obligation, etc.).
- **verification**: Broker asks for more information or identity verification.
- **human_required**: The reply requires manual human review (complex, ambiguous, legal notice).
- **autoresponder**: Automated out-of-office, delivery receipt, or "we received your email".
- **bounce**: Hard bounce — email address does not exist or mailbox is full.
- **unclear**: Cannot confidently classify into any of the above.

Analyze the email content, subject, and any broker context provided.
Respond with ONLY a JSON object on a single line:
{"classification": "<label>", "confidence": <0.0-1.0>, "summary": "<text>", "extracted_fields": {}}

Use extracted_fields to capture relevant data like case numbers, ticket IDs, or dates.
If confidence < 0.4, use "unclear" as the fallback classification.
"""


@dataclass
class ClassificationResult:
    label: str
    event_type: str
    confidence: float
    summary: str
    extracted_fields: dict[str, Any] = field(default_factory=dict)
    usage_record: Any = None
    needs_human_review: bool = False


def build_user_prompt(
    *,
    broker_name: str,
    broker_website: str,
    original_subject: str,
    original_request_snippet: str,
    reply_subject: str,
    reply_body: str,
) -> str:
    """Build the user prompt for the classifier."""
    parts = [f"Broker: {broker_name} ({broker_website})"]

    if original_subject:
        parts.append(f"Original request subject: {original_subject}")

    if original_request_snippet:
        parts.append(f"Original request body (truncated):\n{original_request_snippet[:500]}")

    parts.append(f"\nReply subject: {redact_profile_pii(reply_subject)}")
    parts.append(f"Reply body:\n{redact_profile_pii(reply_body[:2000])}")

    return "\n\n".join(parts)


def _parse_response(response_text: str) -> ClassificationResult:
    """Parse the JSON response from Claude into a ClassificationResult."""
    text = response_text.strip()

    if text.startswith("```"):
        text = text.strip("`").strip()
        if text.startswith("json"):
            text = text[4:].strip()

    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        logger.warning("Failed to parse classifier JSON: %s", text[:200])
        return ClassificationResult(
            label="unclear",
            event_type="HUMAN_ACTION_REQUIRED",
            confidence=0.0,
            summary="Failed to parse classifier response",
            needs_human_review=True,
        )

    label = str(data.get("classification", "unclear")).lower().strip()
    if label not in CLASSIFICATION_LABELS:
        label = "unclear"

    confidence = float(data.get("confidence", 0.0))
    confidence = max(0.0, min(1.0, confidence))

    summary = str(data.get("summary", ""))[:200]
    extracted = data.get("extracted_fields", {})
    if not isinstance(extracted, dict):
        extracted = {}

    event_type = CLASSIFICATION_TO_EVENT.get(label, "HUMAN_ACTION_REQUIRED")
    needs_review = confidence < CONFIDENCE_THRESHOLD_LOW or label == "unclear"

    return ClassificationResult(
        label=label,
        event_type=event_type,
        confidence=confidence,
        summary=summary,
        extracted_fields=extracted,
        needs_human_review=needs_review,
    )


class ReplyClassifier:
    def __init__(
        self,
        *,
        client: LLMClient | None = None,
        cost_tracker: list | None = None,
    ) -> None:
        self._client: LLMClient | None = None
        if client is not None:
            self._client = client
        else:
            from symeraseme.llm.factory import create_llm_client

            self._client = create_llm_client(cost_tracker=cost_tracker)

    def is_available(self) -> bool:
        return self._client is not None and self._client.is_available()

    def classify(
        self,
        *,
        broker_name: str = "",
        broker_website: str = "",
        original_subject: str = "",
        original_request_snippet: str = "",
        reply_subject: str = "",
        reply_body: str = "",
        cache_key: str | None = None,
    ) -> ClassificationResult:
        user_prompt = build_user_prompt(
            broker_name=broker_name,
            broker_website=broker_website,
            original_subject=original_subject,
            original_request_snippet=original_request_snippet,
            reply_subject=reply_subject,
            reply_body=reply_body,
        )

        if self._client is None:
            return ClassificationResult(
                label="unclear",
                event_type="HUMAN_ACTION_REQUIRED",
                confidence=0.0,
                summary="Classifier not initialized",
                needs_human_review=True,
            )

        try:
            response_text, usage = self._client.classify(
                system_prompt=SYSTEM_PROMPT,
                user_prompt=user_prompt,
                cache_key=cache_key or broker_name,
            )
        except LLMClientError as e:
            logger.warning("Classifier API call failed: %s", e)
            return ClassificationResult(
                label="unclear",
                event_type="HUMAN_ACTION_REQUIRED",
                confidence=0.0,
                summary=f"API error: {e}",
                needs_human_review=True,
            )

        result = _parse_response(response_text)
        result.usage_record = usage
        return result

    def close(self) -> None:
        self._client = None
