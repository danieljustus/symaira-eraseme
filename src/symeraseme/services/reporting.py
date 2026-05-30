from __future__ import annotations

import webbrowser
from pathlib import Path
from typing import Any

from symeraseme.core.result_types import CliResult
from symeraseme.core.dashboard import generate_dashboard, get_dashboard_data
from symeraseme.core.reports import generate_report, get_report_data


def handle_generate_dashboard(
    output: str = "report.html",
    auto_open: bool = False,
    auto_refresh: int = 0,
) -> CliResult:
    data = get_dashboard_data()
    html = generate_dashboard(data, auto_refresh_seconds=auto_refresh)
    Path(output).write_text(html)

    result: dict[str, Any] = {
        "output_file": str(Path(output).resolve()),
        "size_bytes": len(html),
        "campaigns": len(data.get("campaigns", [])),
        "requests": data.get("total_requests", 0),
    }

    lines = [
        f"Dashboard generated: {Path(output).resolve()}",
        f"  Size: {len(html)} bytes",
        f"  Campaigns: {len(data.get('campaigns', []))}",
        f"  Requests: {data.get('total_requests', 0)}",
    ]

    if auto_open:
        webbrowser.open(f"file://{Path(output).resolve()}")

    result["message"] = "\n".join(lines)
    return CliResult(success=True, data=result)


def handle_generate_report(
    campaign_id: str | None = None,
    format: str = "html",
    output: str = "",
    all_campaigns: bool = False,
) -> CliResult:
    data = get_report_data(
        campaign_id=campaign_id,
        all_campaigns=all_campaigns,
    )
    report = generate_report(data, format=format)

    if format == "json":
        if output:
            Path(output).write_text(__import__("json").dumps(report, indent=2, default=str))
            return CliResult(
                success=True,
                data={"output_file": str(Path(output).resolve()), "message": f"Report written to {Path(output).resolve()}"},
            )
        return CliResult(
            success=True,
            data={"report": report, "message": "Report generated."},
        )

    if output:
        content = str(report) if isinstance(report, str) else str(report)
        Path(output).write_text(content)
        return CliResult(
            success=True,
            data={"output_file": str(Path(output).resolve()), "message": f"Report written to {Path(output).resolve()}"},
        )

    default_name = f"report-{campaign_id or 'all'}.{format}"
    content = str(report) if isinstance(report, str) else str(report)
    Path(default_name).write_text(content)
    return CliResult(
        success=True,
        data={"output_file": str(Path(default_name).resolve()), "message": f"Report written to {Path(default_name).resolve()}"},
    )
