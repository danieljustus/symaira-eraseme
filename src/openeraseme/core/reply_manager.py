from __future__ import annotations

import logging
from typing import Any

from openeraseme.core.db import get_connection
from openeraseme.core.events import append_event, get_removal_request
from openeraseme.core.projection import upsert_state
from openeraseme.core.templating import render_template

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
    conn = get_connection()
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
        conditions.append(
            "r.id IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NULL)"
        )
    elif status == "sent":
        conditions.append(
            "r.id IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)"
        )
    elif status == "classified":
        conditions.append("r.classified_as IS NOT NULL")
    elif status == "unclassified":
        conditions.append("r.classified_as IS NULL")

    where = " AND ".join(conditions) if conditions else "1=1"
    rows = conn.execute(
        f"""SELECT r.id, r.request_id, r.message_id, r.thread_id,
                   r.received_at, r.from_addr, r.subject, r.snippet,
                   r.classified_as, r.classifier_confidence, r.llm_summary
            FROM inbox_replies r
            WHERE {where}
            ORDER BY r.received_at DESC""",
        params,
    ).fetchall()

    result = [dict(row) for row in rows]
    if result:
        reply_ids = [str(r["id"]) for r in result]
        id_list = ",".join(reply_ids)
        draft_rows = conn.execute(
            f"""SELECT d.id AS draft_id, d.reply_id, d.subject AS draft_subject,
                       d.created_at AS draft_created_at, d.sent_at AS draft_sent_at,
                       d.account
                FROM reply_drafts d
                WHERE d.reply_id IN ({id_list})
                ORDER BY d.created_at DESC""",
        ).fetchall()
        drafts_by_reply: dict[int, dict[str, Any]] = {}
        for dr in draft_rows:
            d = dict(dr)
            reply_id = d.pop("reply_id")
            if reply_id not in drafts_by_reply:
                drafts_by_reply[reply_id] = d

        for r in result:
            draft_info = drafts_by_reply.get(r["id"])
            if draft_info:
                r["draft_id"] = draft_info["draft_id"]
                r["draft_subject"] = draft_info["draft_subject"]
                r["draft_created_at"] = draft_info["draft_created_at"]
                r["draft_sent_at"] = draft_info["draft_sent_at"]
                r["draft_account"] = draft_info["account"]

    return result


def get_reply(reply_id: int) -> dict[str, Any] | None:
    """Get a single inbox reply with full details including draft info."""
    conn = get_connection()
    row = conn.execute(
        """SELECT r.id, r.request_id, r.message_id, r.thread_id,
                  r.received_at, r.from_addr, r.subject, r.snippet,
                  r.classified_as, r.classifier_confidence, r.llm_summary
           FROM inbox_replies r
           WHERE r.id = ?""",
        (reply_id,),
    ).fetchone()

    if row is None:
        return None

    result = dict(row)

    if result.get("request_id"):
        req = get_removal_request(result["request_id"])
        if req:
            result["broker_id"] = req.get("broker_id", "")
            result["campaign_id"] = req.get("campaign_id", "")
            result["jurisdiction"] = req.get("jurisdiction", "")

    draft = conn.execute(
        """SELECT id, draft_body, subject, created_at, sent_at, account
           FROM reply_drafts
           WHERE reply_id = ?
           ORDER BY created_at DESC LIMIT 1""",
        (reply_id,),
    ).fetchone()
    if draft:
        d = dict(draft)
        result["draft_id"] = d["id"]
        result["draft_body"] = d["draft_body"]
        result["draft_subject"] = d["subject"]
        result["draft_created_at"] = d["created_at"]
        result["draft_sent_at"] = d["sent_at"]
        result["draft_account"] = d["account"]

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
    conn = get_connection()

    reply = get_reply(reply_id)
    if reply is None:
        msg = f"Reply #{reply_id} not found"
        raise ValueError(msg)

    request_id = reply.get("request_id")
    if request_id is None:
        msg = f"Reply #{reply_id} is not linked to a removal request"
        raise ValueError(msg)

    existing = conn.execute(
        "SELECT id FROM reply_drafts WHERE reply_id = ? AND sent_at IS NULL",
        (reply_id,),
    ).fetchone()
    if existing:
        draft_id = existing["id"]
        body = conn.execute(
            "SELECT draft_body FROM reply_drafts WHERE id = ?", (draft_id,)
        ).fetchone()["draft_body"]
        return {
            "draft_id": draft_id,
            "reply_id": reply_id,
            "request_id": request_id,
            "draft_body": body,
            "subject": conn.execute(
                "SELECT subject FROM reply_drafts WHERE id = ?", (draft_id,)
            ).fetchone()["subject"],
        }

    req = get_removal_request(request_id)
    broker_id = req.get("broker_id", "") if req else reply.get("broker_id", "")

    broker_name = broker_id
    broker_website = ""
    try:
        from openeraseme.registry.loader import load_broker
        broker = load_broker(broker_id)
        broker_name = broker.name
        broker_website = broker.website
    except Exception:
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
    template_name = template_map.get(
        classification, "gdpr-rebuttal-rejected.en.md.j2"
    )

    try:
        draft_body = render_template(
            template_name,
            profile=None,
            broker_name=broker_name,
            broker_website=broker_website,
            extra_vars=rebuttal_context,
        )
    except Exception:
        logger.warning(
            "Template %s not found, falling back to plain-text rebuttal",
            template_name,
        )
        draft_body = _fallback_rebuttal(
            broker_name=broker_name,
            classification=classification,
            reply_snippet=reply_body,
        )

    draft_subject = f"Re: Data Deletion Request — {broker_name}"

    cur = conn.execute(
        """INSERT INTO reply_drafts
           (reply_id, request_id, draft_body, subject, account)
           VALUES (?, ?, ?, ?, ?)""",
        (reply_id, request_id, draft_body, draft_subject, account),
    )
    conn.commit()
    draft_id = cur.lastrowid

    append_event(
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
    upsert_state(request_id)

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
) -> dict[str, Any]:
    """Send a drafted reply via Himalaya.

    If no draft exists, creates one first.  Idempotent if already sent.
    Returns send result.
    """
    conn = get_connection()

    reply = get_reply(reply_id)
    if reply is None:
        msg = f"Reply #{reply_id} not found"
        raise ValueError(msg)

    request_id = reply.get("request_id")
    if request_id is None:
        msg = f"Reply #{reply_id} is not linked to a removal request"
        raise ValueError(msg)

    draft = conn.execute(
        """SELECT id, draft_body, subject, sent_at
           FROM reply_drafts
           WHERE reply_id = ?
           ORDER BY created_at DESC LIMIT 1""",
        (reply_id,),
    ).fetchone()

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

    from openeraseme.adapters.email.himalaya import HimalayaError, send_message

    from_addr = reply.get("from_addr", "")
    if not from_addr:
        msg = f"Reply #{reply_id} has no sender address"
        raise ValueError(msg)

    try:
        send_message(
            to=from_addr,
            subject=draft_subject,
            body=draft_body,
            account=account,
            config_path=config_path,
        )
    except HimalayaError as e:
        logger.error("Failed to send reply #%d: %s", reply_id, e)
        return {"success": False, "error": str(e), "reply_id": reply_id}

    conn.execute(
        "UPDATE reply_drafts SET sent_at = datetime('now'), account = ? WHERE id = ?",
        (account, draft_id),
    )
    conn.commit()

    append_event(
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
    upsert_state(request_id)

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
) -> str:
    """Plain-text fallback when the rebuttal template is not available."""
    lines = [
        f"Re: Data Deletion Request — {broker_name}",
        "",
    ]
    if classification == "verification":
        lines.extend([
            "Dear Sir or Madam,",
            "",
            "Thank you for your response.",
            "",
            "You have requested additional information to verify my identity.",
            "Please find the requested information attached to this message.",
            "",
            "I look forward to your confirmation that my data has been erased.",
        ])
    else:
        lines.extend([
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
        ])

    lines.extend([
        "",
        "Best regards,",
        "[Your Name]",
    ])
    return "\n".join(lines)
