from __future__ import annotations

import json

from openeraseme.core.db import init_db
from openeraseme.core.deadlines import apply_tick_actions, run_tick


def handle_tick(dry_run: bool = False, output_format: str = "text") -> str:
    init_db()
    actions = run_tick(dry_run=dry_run)

    if output_format == "json":
        return json.dumps(
            {
                "total_actions": len(actions),
                "actions": [a.__dict__ for a in actions],
            },
            indent=2,
            default=str,
        )

    if not actions:
        return "Tick: no actions needed."

    lines = [f"Tick: {len(actions)} action(s)"]
    for a in actions:
        dry_tag = " (DRY RUN)" if a.dry_run else ""
        lines.append(f"  #{a.request_id} [{a.action_type}] {a.description}{dry_tag}")

    if not dry_run:
        results = apply_tick_actions(actions)
        executed = sum(1 for r in results if r["executed"])
        lines.append(f"Executed {executed}/{len(results)} actions.")

    return "\n".join(lines)
