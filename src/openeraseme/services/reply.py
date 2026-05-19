from __future__ import annotations

import json

from openeraseme.adapters.triage.classifier import ReplyClassifier
from openeraseme.adapters.triage.responder import generate_rebuttal
from openeraseme.core.db import get_connection, init_db
from openeraseme.core.events import append_event, get_events, get_removal_request
from openeraseme.core.identity import load_profile, profile_exists
from openeraseme.core.orchestrator import submit_inbox_reply
from openeraseme.core.projection import upsert_state
from openeraseme.registry.loader import load_broker


def handle_classify_reply(
    request_id: int,
    api_key: str | None = None,
    model: str = "claude-3-5-sonnet-latest",
    save: bool = True,
    output_format: str = "text",
) -> str:
    init_db()

    req = get_removal_request(request_id)
    if req is None:
        import typer

        typer.echo(f"Request #{request_id} not found.", err=True)
        raise typer.Exit(1)

    events = get_events(request_id)
    if not events:
        import typer

        typer.echo(f"No events found for request #{request_id}.", err=True)
        raise typer.Exit(1)

    last_event = events[-1]
    payload = last_event.get("payload_json", {})

    broker_id = req.get("broker_id", "")
    try:
        broker = load_broker(broker_id)
    except Exception:
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
        import typer

        typer.echo(f"No unclassified inbox reply found for request #{request_id}.", err=True)
        raise typer.Exit(1)

    classifier = ReplyClassifier(api_key=api_key, model=model)
    if not classifier.is_available():
        import typer

        typer.echo(
            "Anthropic API is not available. Set ANTHROPIC_API_KEY or provide --api-key.",
            err=True,
        )
        raise typer.Exit(1)

    result = classifier.classify(
        broker_name=broker_name,
        broker_website=broker_website,
        original_subject=original_subject,
        original_request_snippet=original_snippet,
        reply_subject=reply["subject"] or "",
        reply_body=reply["snippet"] or "",
        cache_key=f"broker:{broker_id}",
    )

    if output_format == "json":
        output = {
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
            output["usage"] = result.usage_record.record()
        output_str = json.dumps(output, indent=2, default=str)
    else:
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
        output_str = "\n".join(lines)

    if save:
        submit_inbox_reply(
            message_id=str(reply["id"]),
            request_id=request_id,
            from_addr=reply["from_addr"] or "",
            subject=reply["subject"] or "",
            snippet=reply["snippet"] or "",
            classified_as=result.label,
        )
        append_event(
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
        upsert_state(request_id)
        output_str += "\nClassification saved to database."

    return output_str


def handle_generate_rebuttal(
    request_id: int,
    api_key: str | None = None,
    model: str = "claude-3-5-sonnet-latest",
    save: bool = True,
    output_format: str = "text",
) -> str:
    init_db()

    req = get_removal_request(request_id)
    if req is None:
        import typer

        typer.echo(f"Request #{request_id} not found.", err=True)
        raise typer.Exit(1)

    events = get_events(request_id)
    if not events:
        import typer

        typer.echo(f"No events found for request #{request_id}.", err=True)
        raise typer.Exit(1)

    last_event = events[-1]
    payload = last_event.get("payload_json", {})

    broker_id = req.get("broker_id", "")
    try:
        broker = load_broker(broker_id)
    except Exception:
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

    result = generate_rebuttal(
        broker_name=broker_name,
        broker_website=broker_website,
        broker_message=broker_message or "",
        original_request_template=payload.get("template", ""),
        original_request_date=original_request_date,
        profile=profile,
        api_key=api_key,
        model=model,
    )

    if output_format == "json":
        output = {
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
            output["usage"] = result.usage_record.record()
        output_str = json.dumps(output, indent=2)
    else:
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
        output_str = "\n".join(lines)

    if save:
        append_event(
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
        upsert_state(request_id)
        output_str += "\nREBUTTAL_SENT event saved to database."

    return output_str
