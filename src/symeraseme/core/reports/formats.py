"""Report export formats: JSON, CSV, HTML."""

from __future__ import annotations

import csv
import io
import json
from datetime import UTC, datetime
from typing import Any

from jinja2 import Environment, FileSystemLoader, select_autoescape


def export_json(data: dict[str, Any]) -> str:
    return json.dumps(data, indent=2, default=str, ensure_ascii=False)


def export_csv(data: dict[str, Any]) -> str:
    output = io.StringIO()
    writer = csv.writer(output)

    writer.writerow(
        [
            "campaign_id",
            "request_id",
            "broker_id",
            "jurisdiction",
            "channel",
            "status",
            "sent_at",
            "acknowledged_at",
            "resolved_at",
            "deadline_at",
            "reminders_sent",
            "escalation_level",
        ]
    )

    for camp in data.get("campaigns", []):
        for req in camp.get("requests", []):
            writer.writerow(
                [
                    camp["campaign_id"],
                    req.get("id", ""),
                    req.get("broker_id", ""),
                    req.get("jurisdiction", ""),
                    req.get("channel", ""),
                    req.get("current_status", ""),
                    req.get("sent_at", ""),
                    req.get("acknowledged_at", ""),
                    req.get("resolved_at", ""),
                    req.get("deadline_at", ""),
                    req.get("reminders_sent", 0),
                    req.get("escalation_level", 0),
                ]
            )

    return output.getvalue()


def export_html(data: dict[str, Any]) -> str:
    from symeraseme.registry.loader import _registry_dir

    # Resolve the templates dir through the shared registry resolver so this
    # works in both source checkouts and packaged installs, where the registry
    # tree lives at symeraseme/registry_data/ (issue #615).
    templates_dir = _registry_dir() / "templates"
    loader = FileSystemLoader(
        searchpath=[
            str(templates_dir),
        ]
    )
    env = Environment(loader=loader, autoescape=select_autoescape(["html"]))

    template = env.get_template("report.html.j2")
    return template.render(
        data=data,
        now=datetime.now(UTC),
    )


def generate_report(
    data: dict[str, Any],
    format: str = "html",
) -> str | dict[str, Any]:
    """Generate a report in the requested format."""
    format = format.lower()
    if format == "json":
        return export_json(data)
    elif format == "csv":
        return export_csv(data)
    elif format == "html":
        return export_html(data)
    else:
        msg = f"Unsupported format: {format}. Choose html, json, or csv."
        raise ValueError(msg)
