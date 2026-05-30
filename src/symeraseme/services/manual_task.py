from __future__ import annotations

from symeraseme.cli.console import render_error
from symeraseme.core.result_types import CliResult
from symeraseme.core.db import init_db
from symeraseme.core.manual_fallback import get_manual_task, list_manual_tasks, resume_from_manual


def handle_manual_tasks_list(
    status: str | None = None,
    request_id: int | None = None,
) -> CliResult:
    init_db()
    tasks = list_manual_tasks(status=status, request_id=request_id)

    if not tasks:
        return CliResult(success=True, data={"tasks": tasks, "message": "No manual tasks found."})

    lines = [f"Manual tasks ({len(tasks)}):"]
    for t in tasks:
        status_str = t.get("status", "unknown")
        task_id = t.get("id", "?")
        broker = t.get("broker_name", t.get("broker_id", "?"))
        reason = t.get("reason", "?")
        created = t.get("created_at", "?")
        lines.append(f"  #{task_id} [{status_str}] {broker} ({reason}) @ {created}")

    return CliResult(success=True, data={"tasks": tasks, "message": "\n".join(lines)})


def handle_manual_tasks_show(task_id: int) -> CliResult:
    init_db()
    task = get_manual_task(task_id)

    if task is None:
        render_error(
            f"Manual task #{task_id} not found. "
            "Run 'symeraseme manual-tasks list' to see available tasks."
        )

    lines = [f"Manual task #{task_id}:"]
    lines.append(f"  Broker:     {task.get('broker_name', '?')} ({task.get('broker_id', '?')})")
    lines.append(f"  URL:        {task.get('form_url', '?')}")
    lines.append(f"  Reason:     {task.get('reason', '?')}")
    lines.append(f"  Status:     {task.get('status', '?')}")
    lines.append(f"  Created:    {task.get('created_at', '?')}")
    if task.get("completed_at"):
        lines.append(f"  Completed:  {task['completed_at']}")
    if task.get("screenshot_path"):
        lines.append(f"  Screenshot: {task['screenshot_path']}")
    if task.get("html_snapshot_path"):
        lines.append(f"  HTML:       {task['html_snapshot_path']}")
    lines.append(f"\nInstructions:\n{task.get('instructions', '')}")
    if task.get("notes"):
        lines.append(f"\nNotes: {task['notes']}")

    task["message"] = "\n".join(lines)
    return CliResult(success=True, data=task)


def handle_manual_tasks_complete(
    task_id: int,
    notes: str = "",
) -> CliResult:
    init_db()
    result = resume_from_manual(task_id, notes=notes, completed=True)

    if result is None:
        render_error(
            f"Manual task #{task_id} not found. "
            "Run 'symeraseme manual-tasks list' to see available tasks."
        )

    return CliResult(
        success=True,
        data={"task_id": task_id, "message": f"Manual task #{task_id} marked as completed."},
    )


def handle_manual_tasks_cleanup(
    dry_run: bool = False,
) -> CliResult:
    """Remove old screenshot and HTML snapshot files from manual tasks directory."""
    from pathlib import Path

    from symeraseme.core.manual_fallback import MANUAL_TASKS_DIR

    tasks_dir = Path(MANUAL_TASKS_DIR).expanduser()
    if not tasks_dir.exists():
        return CliResult(
            success=True,
            data={"message": "No manual tasks directory found — nothing to clean up."},
        )

    removed = 0
    skipped = 0
    for path in tasks_dir.iterdir():
        if path.is_file() and path.suffix in (".png", ".html", ".json"):
            if dry_run:
                skipped += 1
            else:
                path.unlink()
                removed += 1

    from typing import Any

    result: dict[str, Any] = {"removed": removed, "skipped": skipped, "dry_run": dry_run}
    if dry_run:
        result["message"] = f"Would remove {skipped} artifact(s) from {tasks_dir}. Use --yes to confirm."
    else:
        result["message"] = f"Removed {removed} artifact(s) from {tasks_dir}."

    return CliResult(success=True, data=result)
