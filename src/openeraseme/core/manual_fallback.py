"""Manual fallback for complex or unhandled web forms.

When Playwright encounters a form it cannot handle automatically (unknown
CAPTCHA type, timeout, login wall, AJAX-heavy fields, multi-step wizards
exceeding limits), this module captures the form state, creates a manual
completion task, and stores it in the event store so the user can complete
the process manually.
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from openeraseme.core.db import get_connection

logger = logging.getLogger(__name__)

MANUAL_TASKS_DIR = "~/.local/share/openeraseme/manual_tasks"

FALLBACK_REASONS = frozenset(
    {
        "unknown_captcha",
        "captcha_failed",
        "timeout",
        "login_required",
        "multi_step_exceeded",
        "dynamic_form",
        "unknown_field",
        "assertion_failed",
        "generic_error",
    }
)


@dataclass
class FormState:
    url: str
    screenshot_path: str | None = None
    html_snapshot: str = ""
    form_fields: dict[str, str] = field(default_factory=dict)
    field_selectors: list[str] = field(default_factory=list)
    error_message: str = ""
    reason: str = ""
    step_index: int = 0
    total_steps: int = 0
    broker_name: str = ""
    broker_id: str = ""


@dataclass
class ManualTask:
    id: int = 0
    request_id: int | None = None
    broker_id: str = ""
    broker_name: str = ""
    form_url: str = ""
    reason: str = ""
    instructions: str = ""
    screenshot_path: str = ""
    html_snapshot_path: str = ""
    form_fields_json: str = ""
    status: str = "pending"
    created_at: str = ""
    completed_at: str | None = None
    notes: str = ""


def _tasks_dir() -> Path:
    tasks_dir = Path(MANUAL_TASKS_DIR).expanduser()
    tasks_dir.mkdir(parents=True, exist_ok=True)
    return tasks_dir


def _task_file_path(task_id: int) -> Path:
    return _tasks_dir() / f"manual_task_{task_id}.json"


def capture_form_state(
    page: Any,
    *,
    url: str = "",
    reason: str = "generic_error",
    error_message: str = "",
    step_index: int = 0,
    total_steps: int = 0,
    broker_name: str = "",
    broker_id: str = "",
    screenshot_dir: str | Path | None = None,
) -> FormState:
    """Capture the current state of a web form for manual fallback.

    Args:
        page: Playwright page object (or any object with url, content, title methods).
        url: Form URL override (uses page.url if not provided).
        reason: Failure reason code from FALLBACK_REASONS.
        error_message: Human-readable error description.
        step_index: Step index where failure occurred.
        total_steps: Total number of steps in the form.
        broker_name: Name of the data broker.
        broker_id: ID of the data broker.
        screenshot_dir: Directory to save screenshot.
    """
    captured_url = url or getattr(page, "url", "")
    html_snapshot = ""
    form_fields: dict[str, str] = {}
    field_selectors: list[str] = []
    screenshot_path: str | None = None

    async def _capture() -> None:
        nonlocal html_snapshot, form_fields, field_selectors, screenshot_path
        try:
            html_snapshot = await _async_get_content(page)
        except Exception as e:
            logger.warning("Failed to capture HTML snapshot: %s", e)

        try:
            form_fields, field_selectors = await _async_extract_form_fields(page)
        except Exception as e:
            logger.warning("Failed to extract form fields: %s", e)

        if screenshot_dir:
            try:
                screenshot_path = await _async_save_screenshot(
                    page, Path(screenshot_dir), "manual_fallback"
                )
            except Exception as e:
                logger.warning("Failed to save screenshot: %s", e)

    asyncio.run(_capture())

    return FormState(
        url=captured_url,
        screenshot_path=screenshot_path,
        html_snapshot=html_snapshot[:5000],
        form_fields=form_fields,
        field_selectors=field_selectors,
        error_message=error_message,
        reason=reason if reason in FALLBACK_REASONS else "generic_error",
        step_index=step_index,
        total_steps=total_steps,
        broker_name=broker_name,
        broker_id=broker_id,
    )


async def _async_get_content(page: Any) -> str:
    """Get page content from an async Playwright page."""
    return await page.content()


async def _async_extract_form_fields(page: Any) -> tuple[dict[str, str], list[str]]:
    """Extract form field names and current values from a Playwright page."""
    field_info = await page.evaluate(
        """() => {
            const fields = {};
            const selectors = [];
            document.querySelectorAll('input, select, textarea').forEach(el => {
                if (el.name || el.id) {
                    const key = el.name || el.id;
                    fields[key] = el.value || '';
                    const selector = el.id ? '#' + el.id :
                        'input[name="' + el.name + '"]'
                    selectors.push(selector);
                }
            });
            return { fields: JSON.stringify(fields), selectors: JSON.stringify(selectors) };
        }"""
    )
    fields = json.loads(field_info.get("fields", "{}"))
    selectors = json.loads(field_info.get("selectors", "[]"))
    return fields, selectors


async def _async_save_screenshot(page: Any, directory: Path, name: str) -> str:
    """Save a page screenshot."""
    timestamp = time.strftime("%Y%m%d-%H%M%S")
    safe_name = "".join(c if c.isalnum() or c in "-_" else "_" for c in name)
    filename = f"{timestamp}_{safe_name}.png"
    path = directory / filename
    await page.screenshot(path=str(path), full_page=True)
    return str(path)


def _instructions_for_reason(reason: str, broker_name: str, form_url: str) -> str:
    """Generate human-readable instructions based on the fallback reason."""
    instructions: dict[str, str] = {
        "unknown_captcha": (
            f"The web form for {broker_name} has an unknown CAPTCHA type that "
            "could not be solved automatically. Please visit the URL below and "
            "complete the CAPTCHA manually, then submit the form."
        ),
        "captcha_failed": (
            f"The CAPTCHA solver failed for {broker_name}'s opt-out form. "
            "Please visit the URL below and complete the CAPTCHA manually."
        ),
        "timeout": (
            f"The web form for {broker_name} timed out during submission. "
            "This may indicate a slow server or a multi-step process. "
            "Please visit the URL below and complete the opt-out process manually."
        ),
        "login_required": (
            f"The web form for {broker_name} requires login or authentication. "
            "Automatic form filling cannot proceed. Please log in and complete "
            "the opt-out process manually."
        ),
        "multi_step_exceeded": (
            f"The web form for {broker_name} requires more steps than the "
            "configured limit. Please visit the URL below and follow the "
            "opt-out process to completion."
        ),
        "dynamic_form": (
            f"The web form for {broker_name} uses dynamic JavaScript fields "
            "that could not be filled automatically. Please visit the URL and "
            "complete the form manually."
        ),
        "unknown_field": (
            f"The web form for {broker_name} contains unknown fields that "
            "could not be mapped from the identity profile. "
            "Please visit the URL and complete the form manually."
        ),
        "assertion_failed": (
            f"The web form for {broker_name} was submitted but the expected "
            "confirmation message was not displayed. Please visit the URL "
            "and verify whether the opt-out was successful."
        ),
        "generic_error": (
            f"An unexpected error occurred while processing the web form for "
            f"{broker_name}. Please visit the URL below and complete the "
            "opt-out process manually."
        ),
    }
    return instructions.get(
        reason,
        f"Please complete the opt-out process for {broker_name} manually "
        f"by visiting the URL below.",
    )


def create_manual_task(
    *,
    request_id: int | None = None,
    broker_id: str = "",
    broker_name: str = "",
    form_url: str = "",
    reason: str = "generic_error",
    screenshot_path: str = "",
    html_snapshot: str = "",
    form_fields: dict[str, str] | None = None,
    step_index: int = 0,
    total_steps: int = 0,
    error_message: str = "",
    extra_instructions: str = "",
) -> ManualTask:
    """Create a manual completion task and store it.

    Also appends a HUMAN_ACTION_REQUIRED event to the request event store.
    """
    if reason not in FALLBACK_REASONS:
        reason = "generic_error"

    instructions = _instructions_for_reason(reason, broker_name, form_url)
    if extra_instructions:
        instructions = f"{instructions}\n\n{extra_instructions}"

    now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S")

    # Save HTML snapshot to file if provided
    html_path = ""
    if html_snapshot:
        try:
            html_path = str(_tasks_dir() / f"snapshot_{int(time.time())}.html")
            Path(html_path).write_text(html_snapshot)
        except OSError as e:
            logger.warning("Failed to save HTML snapshot: %s", e)

    conn = get_connection()
    cur = conn.execute(
        """INSERT INTO manual_tasks
           (request_id, broker_id, broker_name, form_url, reason, instructions,
            screenshot_path, html_snapshot_path, form_fields_json, status)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')""",
        (
            request_id,
            broker_id,
            broker_name,
            form_url,
            reason,
            instructions,
            screenshot_path,
            html_path,
            json.dumps(form_fields or {}),
        ),
    )
    conn.commit()
    task_id: int = cur.lastrowid  # type: ignore[assignment]

    # Append HUMAN_ACTION_REQUIRED event
    if request_id:
        try:
            from openeraseme.core.events import append_event

            append_event(
                request_id,
                "HUMAN_ACTION_REQUIRED",
                payload={
                    "manual_task_id": task_id,
                    "reason": reason,
                    "form_url": form_url,
                    "instructions": instructions,
                    "screenshot_path": screenshot_path,
                    "broker_name": broker_name,
                    "step_index": step_index,
                    "total_steps": total_steps,
                    "error_message": error_message,
                },
                source="system",
            )
        except Exception as e:
            logger.warning("Failed to append event for manual task: %s", e)

    return ManualTask(
        id=task_id,
        request_id=request_id,
        broker_id=broker_id,
        broker_name=broker_name,
        form_url=form_url,
        reason=reason,
        instructions=instructions,
        screenshot_path=screenshot_path,
        html_snapshot_path=html_path,
        form_fields_json=json.dumps(form_fields or {}),
        status="pending",
        created_at=now,
    )


def resume_from_manual(
    task_id: int,
    *,
    notes: str = "",
    completed: bool = True,
) -> ManualTask | None:
    """Mark a manual task as completed (or cancelled) after user action."""
    conn = get_connection()
    row = conn.execute("SELECT * FROM manual_tasks WHERE id = ?", (task_id,)).fetchone()

    if row is None:
        logger.warning("Manual task %d not found", task_id)
        return None

    new_status = "completed" if completed else "cancelled"
    now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S")

    conn.execute(
        "UPDATE manual_tasks SET status = ?, completed_at = ?, notes = ? WHERE id = ?",
        (new_status, now, notes, task_id),
    )
    conn.commit()

    # Append NOTE_ADDED event for the request
    request_id = row["request_id"]
    if request_id:
        try:
            from openeraseme.core.events import append_event

            append_event(
                request_id,
                "NOTE_ADDED",
                payload={
                    "note": f"Manual task #{task_id} {new_status}: {notes}",
                    "manual_task_id": task_id,
                    "form_url": row["form_url"],
                },
                source="user",
            )
        except Exception as e:
            logger.warning("Failed to append resume event: %s", e)

    return ManualTask(
        id=task_id,
        request_id=request_id,
        broker_id=row["broker_id"],
        broker_name=row["broker_name"],
        form_url=row["form_url"],
        reason=row["reason"],
        instructions=row["instructions"],
        screenshot_path=row["screenshot_path"],
        html_snapshot_path=row["html_snapshot_path"],
        form_fields_json=row["form_fields_json"],
        status=new_status,
        created_at=row["created_at"],
        completed_at=now,
        notes=notes,
    )


def list_manual_tasks(
    *,
    status: str | None = None,
    request_id: int | None = None,
) -> list[dict[str, Any]]:
    """List manual tasks, optionally filtered by status or request ID."""
    conn = get_connection()
    conditions: list[str] = []
    params: list[Any] = []

    if status:
        conditions.append("status = ?")
        params.append(status)
    if request_id is not None:
        conditions.append("request_id = ?")
        params.append(request_id)

    where = " AND ".join(conditions) if conditions else "1=1"
    rows = conn.execute(
        f"SELECT * FROM manual_tasks WHERE {where} ORDER BY created_at DESC",
        params,
    ).fetchall()
    return [dict(r) for r in rows]


def get_manual_task(task_id: int) -> dict[str, Any] | None:
    """Get a single manual task by ID."""
    conn = get_connection()
    row = conn.execute("SELECT * FROM manual_tasks WHERE id = ?", (task_id,)).fetchone()
    return dict(row) if row else None
