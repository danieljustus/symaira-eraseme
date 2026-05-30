from __future__ import annotations

import logging

from symeraseme.adapters.triage.classifier import ReplyClassifier
from symeraseme.adapters.triage.responder import generate_rebuttal
from symeraseme.adapters.triage.scrubber import grant_llm_consent, llm_consent_granted
from symeraseme.cli.console import render_error
from symeraseme.core.result_types import CliResult
from symeraseme.core.db import get_connection, init_db
from symeraseme.core.events import get_events, get_removal_request
from symeraseme.core.identity import load_profile, profile_exists
from symeraseme.core.orchestrator import submit_inbox_reply
from symeraseme.core.projection import append_event_and_project
from symeraseme.registry.loader import load_broker

logger = logging.getLogger(__name__)


def _ensure_llm_consent(yes: bool = False) -> None:
    if llm_consent_granted():
        return
    if yes:
        grant_llm_consent()
        return
    import typer

    typer.echo(
        "WARNING: LLM operations may send PII (email addresses, phone numbers, SSNs) "
        "to third-party LLM providers. A PII scrubber is active, but network "
        "transmission of scrubbed metadata still occurs.",
        err=True,
    )
    granted = typer.confirm("Do you consent to sending this data to the LLM provider?")
    if not granted:
        render_error("LLM consent denied. Use --yes to grant non-interactively.")
    grant_llm_consent()
    typer.echo("LLM consent granted. This will not be asked again.")


def handle_classify_reply(
    request_id: int,
    provider: str | None = None,
    model: str | None = None,
    save: bool = True,
    yes: bool = False,
) -> CliResult:
    _ensure_llm_consent(yes=yes)
    init_db()

    req = get_removal_request(request_id)
    if req is None:
        render_error(
            f"Request #{request_id} not found. "
            "Run 'symeraseme requests list' to see available requests."
        )

    events = get_events(request_id)
    if not events:
        render_error(
            f"No events found for request #{request_id}. "
            "Events are created when a request is planned or sent."
        )

    last_event = events[-1]
    payload = last_event.get("payload_json", {})

    broker_id = req.get("broker_id", "")
    try:
        broker = load_broker(broker_id)
    except (FileNotFoundError, ValueError, RuntimeError, OSError, LookupError):
        logger.warning("Failed to load broker %s", broker_id)
        broker = None

    broker_name = broker.name if broker else broker_id
    broker_website = broker.website if broker else ""
    original_subject = f"Data Deletion Request — {broker_name}"
    original_snippet = payload.get("template", "")

    conn = get_connection()
    reply = conn.execute(
        "SELECT id, subject, snippet, from_addr FROM inbox_replies "
        "WHERE request_id = ? AND classified_as IS NULL "
        "ORDER BY received_at DESC LIMIT 1",
        (request_id,),
    ).fetchone()

    if reply is None:
        render_error(
            f"No unclassified inbox reply found for request #{request_id}. "
            "Run 'symeraseme poll-inbox' to fetch new replies first."
        )

    from symeraseme.llm.factory import create_llm_client

    client = create_llm_client(provider=provider, model=model)
    classifier = ReplyClassifier(client=client)
    if not classifier.is_available():
        render_error(
            "LLM provider not available. Check SYMERASEME_LLM_PROVIDER"
            " and provider-specific API key."
        )

    result = classifier.classify(
        broker_name=broker_name,
        broker_website=broker_website,
        original_subject=original_subject,
        original_request_snippet=original_snippet,
        reply_subject=reply["subject"] or "",
        reply_body=reply["snippet"] or "",
        cache_key=f"broker:{broker_id}",
    )

    data = {
        "request_id": request_id,
        "reply_id": reply["id"],
        "classification": result.label,
        "event_type": result.event_type,
        "confidence": result.confidence,
        "summary": result.summary,
        "needs_human_review": result.needs_human_review,
        "extracted_fields": result.extracted_fields,
    }
    if result.usage_record:
        data["usage"] = result.usage_record.record()

    lines = [f"Classification for request #{request_id}:"]
    lines.append(f"  Label:      {result.label}")
    lines.append(f"  Event:      {result.event_type}")
    lines.append(f"  Confidence: {result.confidence:.2f}")
    lines.append(f"  Summary:    {result.summary}")
    if result.extracted_fields:
        lines.append(f"  Extracted:  {result.extracted_fields}")
    if result.needs_human_review:
        lines.append("  *** Needs human review ***")
    if result.usage_record:
        usage = result.usage_record.record()
        lines.append(
            f"  Cost:       ${usage['cost']:.6f}"
            f" ({usage['input_tokens']} in / {usage['output_tokens']} out)"
        )

    if save:
        submit_inbox_reply(
            message_id=str(reply["id"]),
            request_id=request_id,
            from_addr=reply["from_addr"] or "",
            subject=reply["subject"] or "",
            snippet=reply["snippet"] or "",
            classified_as=result.label,
        )
        append_event_and_project(
            request_id,
            result.event_type,
            payload={
                "classification": result.label,
                "confidence": result.confidence,
                "summary": result.summary,
                "extracted_fields": result.extracted_fields,
                "reply_id": reply["id"],
            },
            source="system",
        )
        lines.append("Classification saved to database.")

    data["message"] = "\n".join(lines)
    return CliResult(success=True, data=data)


def handle_generate_rebuttal(
    request_id: int,
    provider: str | None = None,
    model: str | None = None,
    save: bool = True,
    yes: bool = False,
) -> CliResult:
    _ensure_llm_consent(yes=yes)
    init_db()

    req = get_removal_request(request_id)
    if req is None:
        render_error(
            f"Request #{request_id} not found. "
            "Run 'symeraseme requests list' to see available requests."
        )

    events = get_events(request_id)
    if not events:
        render_error(
            f"No events found for request #{request_id}. "
            "Events are created when a request is planned or sent."
        )

    last_event = events[-1]
    payload = last_event.get("payload_json", {})

    broker_id = req.get("broker_id", "")
    try:
        broker = load_broker(broker_id)
    except (FileNotFoundError, ValueError, RuntimeError, OSError, LookupError):
        logger.warning("Failed to load broker %s", broker_id)
        broker = None

    broker_name = broker.name if broker else broker_id
    broker_website = broker.website if broker else ""

    conn = get_connection()
    reply = conn.execute(
        "SELECT id, subject, snippet, from_addr FROM inbox_replies "
        "WHERE request_id = ? ORDER BY received_at DESC LIMIT 1",
        (request_id,),
    ).fetchone()

    broker_message = reply["snippet"] if reply else payload.get("template", "")
    original_request_date = last_event.get("occurred_at", "")

    profile = load_profile() if profile_exists() else None

    from symeraseme.llm.factory import create_llm_client

    client = create_llm_client(provider=provider, model=model)
    result = generate_rebuttal(
        broker_name=broker_name,
        broker_website=broker_website,
        broker_message=broker_message or "",
        original_request_template=payload.get("template", ""),
        original_request_date=original_request_date,
        profile=profile,
        client=client,
    )

    data = {
        "request_id": request_id,
        "template_name": result.template_name,
        "label": result.label,
        "jurisdiction": result.jurisdiction,
        "rejection_classification": result.rejection_classification,
        "confidence": result.confidence,
        "needs_human_review": result.needs_human_review,
        "llm_used": result.llm_used,
        "rebuttal_body": result.rebuttal_body,
    }
    if result.usage_record:
        data["usage"] = result.usage_record.record()

    lines = [f"Rebuttal for request #{request_id}:"]
    lines.append(f"  Template:   {result.label}")
    lines.append(f"  Jurisdiction: {result.jurisdiction}")
    lines.append(f"  Confidence: {result.confidence:.2f}")
    lines.append(f"  LLM used:   {result.llm_used}")
    if result.needs_human_review:
        lines.append("  *** Needs human review ***")
    if result.usage_record:
        usage = result.usage_record.record()
        lines.append(
            f"  Cost:       ${usage['cost']:.6f}"
            f" ({usage['input_tokens']} in / {usage['output_tokens']} out)"
        )
    lines.append("")
    lines.append(result.rebuttal_body)

    if save:
        append_event_and_project(
            request_id,
            "REBUTTAL_SENT",
            payload={
                "template_name": result.template_name,
                "rejection_classification": result.rejection_classification,
                "confidence": result.confidence,
                "llm_used": result.llm_used,
                "broker_message_snippet": (broker_message or "")[:200],
            },
            source="system",
        )
        lines.append("REBUTTAL_SENT event saved to database.")

    data["message"] = "\n".join(lines)
    return CliResult(success=True, data=data)

