from __future__ import annotations

import logging
from typing import Any

from symeraseme.core.events import get_removal_request
from symeraseme.core.identity import load_profile, profile_exists
from symeraseme.core.projection import append_event_and_project
from symeraseme.core.templating import render_template

logger = logging.getLogger(__name__)

CLASSIFICATIONS_NEEDING_REPLY = frozenset(
    {
        "rejected",
        "verification",
        "human_required",
        "unclear",
    }
)


def list_replies(
    *,
    status: str | None = None,
    request_id: int | None = None,
) -> list[dict[str, Any]]:
    """List inbox replies, optionally filtered by status.

    Status values:
      - ``needs_reply``: classified replies that need a response
      - ``needs_verification``: replies classified as ``verification``
      - ``drafted``: has a draft but not yet sent
      - ``sent``: draft has been sent
      - ``classified``: has a classification label
      - ``unclassified``: no classification yet
      - ``all`` / ``None``: no filter
    """
    from symeraseme.core.repositories.replies import list_replies as _list_replies

    conditions: list[str] = []
    params: list[str] = []

    if request_id is not None:
        conditions.append("r.request_id = ?")
        params.append(str(request_id))

    if status == "needs_reply":
        placeholders = ",".join("?" for _ in CLASSIFICATIONS_NEEDING_REPLY)
        conditions.append(f"r.classified_as IN ({placeholders})")
        params.extend(CLASSIFICATIONS_NEEDING_REPLY)
        conditions.append(
            "r.id NOT IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)"
        )
    elif status == "needs_verification":
        conditions.append("r.classified_as = ?")
        params.append("verification")
        conditions.append(
            "r.id NOT IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)"
        )
    elif status == "drafted":
        conditions.append("r.id IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NULL)")
    elif status == "sent":
        conditions.append("r.id IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)")
    elif status == "classified":
        conditions.append("r.classified_as IS NOT NULL")
    elif status == "unclassified":
        conditions.append("r.classified_as IS NULL")

    where = " AND ".join(conditions) if conditions else "1=1"
    return _list_replies(where, params)


def get_reply(reply_id: int) -> dict[str, Any] | None:
    """Get a single inbox reply with full details including draft info."""
    from symeraseme.core.repositories.replies import get_reply as _get_reply

    result = _get_reply(reply_id)
    if result is None:
        return None

    if result.get("request_id"):
        req = get_removal_request(result["request_id"])
        if req:
            result["broker_id"] = req.get("broker_id", "")
            result["campaign_id"] = req.get("campaign_id", "")
            result["jurisdiction"] = req.get("jurisdiction", "")

    return result


def draft_reply(
    reply_id: int,
    *,
    account: str | None = None,
) -> dict[str, Any]:
    """Generate a draft response for a broker reply using the rebuttal template.

    Returns the draft metadata.  If a draft already exists for this reply it is
    returned instead of creating a new one (idempotent).
    """
    from symeraseme.core.repositories.replies import (
        get_draft_detail,
        get_existing_draft_id,
    )

    reply = get_reply(reply_id)
    if reply is None:
        msg = f"Reply #{reply_id} not found"
        raise ValueError(msg)

    request_id = reply.get("request_id")
    if request_id is None:
        msg = f"Reply #{reply_id} is not linked to a removal request"
        raise ValueError(msg)

    existing_draft_id = get_existing_draft_id(reply_id)
    if existing_draft_id is not None:
        draft_info = get_draft_detail(existing_draft_id)
        if draft_info:
            return {
                "draft_id": existing_draft_id,
                "reply_id": reply_id,
                "request_id": request_id,
                "draft_body": draft_info["draft_body"],
                "subject": draft_info["subject"],
            }

    req = get_removal_request(request_id)
    broker_id = req.get("broker_id", "") if req else reply.get("broker_id", "")

    broker_name = broker_id
    broker_website = ""
    try:
        from symeraseme.registry.loader import load_broker

        broker = load_broker(broker_id)
        broker_name = broker.name
        broker_website = broker.website
    except (FileNotFoundError, ValueError, RuntimeError, OSError, LookupError):
        pass

    classification = reply.get("classified_as", "unclear")
    reply_body = reply.get("snippet", "")

    rebuttal_context = {
        "classification": classification,
        "broker_reply_snippet": reply_body[:500],
        "request_id": str(request_id),
        "reply_id": str(reply_id),
    }

    template_map = {
        "rejected": "gdpr-rebuttal-rejected.en.md.j2",
        "verification": "gdpr-rebuttal-verification.en.md.j2",
        "human_required": "gdpr-rebuttal-rejected.en.md.j2",
        "unclear": "gdpr-rebuttal-rejected.en.md.j2",
    }
    template_name = template_map.get(classification, "gdpr-rebuttal-rejected.en.md.j2")

    try:
        identity_profile = load_profile() if profile_exists() else None
    except (FileNotFoundError, OSError, ValueError):
        logger.warning("Failed to load identity profile, proceeding without it")
        identity_profile = None

    try:
        draft_body = render_template(
            template_name,
            profile=identity_profile,
            broker_name=broker_name,
            broker_website=broker_website,
            extra_vars=rebuttal_context,
        )
    except (FileNotFoundError, OSError, ValueError, LookupError):
        logger.warning(
            "Template %s not found or renderable, using fallback",
            template_name,
        )
        user_name = identity_profile.full_name if identity_profile else "Your Name"
        draft_body = _fallback_rebuttal(
            broker_name=broker_name,
            classification=classification,
            reply_snippet=reply_body,
            user_name=user_name,
        )

    draft_subject = f"Re: Data Deletion Request — {broker_name}"

    from symeraseme.core.repositories.replies import insert_reply_draft

    draft_id = insert_reply_draft(
        reply_id=reply_id,
        request_id=request_id,
        draft_body=draft_body,
        draft_subject=draft_subject,
        account=account,
    )

    append_event_and_project(
        request_id,
        "REPLY_DRAFTED",
        payload={
            "reply_id": reply_id,
            "draft_id": draft_id,
            "classification": classification,
            "broker": broker_name,
            "template": template_name,
        },
        source="system",
    )

    return {
        "draft_id": draft_id,
        "reply_id": reply_id,
        "request_id": request_id,
        "draft_body": draft_body,
        "subject": draft_subject,
    }


def send_reply(
    reply_id: int,
    *,
    account: str | None = None,
    config_path: str | None = None,
    dry_run: bool = False,
    email_sender=None,
) -> dict[str, Any]:
    """Send a drafted reply via the configured email backend (SMTP by default).

    If no draft exists, creates one first.  Idempotent if already sent.
    Returns send result.
    """
    from symeraseme.core.repositories.replies import get_latest_draft, mark_draft_sent

    reply = get_reply(reply_id)
    if reply is None:
        msg = f"Reply #{reply_id} not found"
        raise ValueError(msg)

    request_id = reply.get("request_id")
    if request_id is None:
        msg = f"Reply #{reply_id} is not linked to a removal request"
        raise ValueError(msg)

    draft = get_latest_draft(reply_id)

    if draft is None:
        draft_result = draft_reply(reply_id, account=account)
        draft_body = draft_result["draft_body"]
        draft_subject = draft_result["subject"]
        draft_id = draft_result["draft_id"]
    else:
        draft_body = draft["draft_body"]
        draft_subject = draft["subject"]
        draft_id = draft["id"]

        if draft["sent_at"] is not None:
            return {
                "success": True,
                "already_sent": True,
                "reply_id": reply_id,
                "draft_id": draft_id,
                "request_id": request_id,
            }

    if dry_run:
        return {
            "success": True,
            "dry_run": True,
            "reply_id": reply_id,
            "draft_id": draft_id,
            "request_id": request_id,
            "to": reply.get("from_addr", ""),
            "subject": draft_subject,
            "body": draft_body,
        }

    from_addr = reply.get("from_addr", "")
    if not from_addr:
        msg = f"Reply #{reply_id} has no sender address"
        raise ValueError(msg)

    if email_sender is None:
        from symeraseme.adapters.email.himalaya import send_email as default_email_sender

        email_sender = default_email_sender

    try:
        email_sender(
            to=from_addr,
            subject=draft_subject,
            body=draft_body,
            account=account,
            config_path=config_path,
        )
    except (OSError, ValueError, RuntimeError) as e:
        logger.error("Failed to send reply #%d: %s", reply_id, e)
        return {"success": False, "error": str(e), "reply_id": reply_id}

    mark_draft_sent(draft_id, account)

    append_event_and_project(
        request_id,
        "REBUTTAL_SENT",
        payload={
            "reply_id": reply_id,
            "draft_id": draft_id,
            "to": from_addr,
            "subject": draft_subject,
        },
        source="system",
    )

    return {
        "success": True,
        "reply_id": reply_id,
        "draft_id": draft_id,
        "request_id": request_id,
        "to": from_addr,
        "subject": draft_subject,
    }


def _fallback_rebuttal(
    *,
    broker_name: str,
    classification: str,
    reply_snippet: str,
    user_name: str = "Your Name",
) -> str:
    """Plain-text fallback when the rebuttal template is not available."""
    lines = [
        f"Re: Data Deletion Request — {broker_name}",
        "",
    ]
    if classification == "verification":
        lines.extend(
            [
                "Dear Sir or Madam,",
                "",
                "Thank you for your response.",
                "",
                "You have requested additional information to verify my identity.",
                "Please find the requested information attached to this message.",
                "",
                "I look forward to your confirmation that my data has been erased.",
            ]
        )
    else:
        lines.extend(
            [
                "Dear Sir or Madam,",
                "",
                "I am writing in response to your reply regarding my data deletion request.",
                "",
                "You responded with the following:",
                "",
                f"{reply_snippet}",
                "",
                "I respectfully disagree with your assessment and reaffirm my request",
                "for the erasure of my personal data under applicable data protection law.",
                "",
                "Please confirm the deletion at your earliest convenience.",
            ]
        )

    lines.extend(
        [
            "",
            "Best regards,",
            user_name,
        ]
    )
    return "\n".join(lines)
