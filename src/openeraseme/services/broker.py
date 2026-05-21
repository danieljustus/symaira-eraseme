"""Broker discovery CLI handlers (brokers list, brokers show)."""

from __future__ import annotations

import json

import typer

from openeraseme.registry.loader import load_all_brokers, load_broker
from openeraseme.registry.schema import EmailOptOut, WebFormOptOut


def handle_brokers_list(
    jurisdiction: str | None = None,
    priority: str | None = None,
    category: str | None = None,
    include_disabled: bool = False,
    output_format: str = "text",
) -> str:
    """List brokers in the registry, optionally filtered.

    Disabled brokers (e.g. with unverified CAPTCHA sitekeys) are excluded
    unless ``include_disabled=True``.
    """
    brokers = load_all_brokers(
        jurisdiction=jurisdiction,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
    )

    if output_format == "json":
        payload = {
            "schema_version": 1,
            "filters": {
                "jurisdiction": jurisdiction,
                "priority": priority,
                "category": category,
                "include_disabled": include_disabled,
            },
            "count": len(brokers),
            "brokers": [
                {
                    "id": b.id,
                    "name": b.name,
                    "website": b.website,
                    "category": b.category.value,
                    "jurisdictions": b.jurisdictions,
                    "laws": [law.value for law in b.laws],
                    "priority": b.priority.value,
                    "data_sensitivity": b.data_sensitivity,
                    "disabled": b.disabled,
                    "opt_out_channels": [ch.type for ch in b.opt_out],
                }
                for b in brokers
            ],
        }
        return json.dumps(payload, indent=2, default=str)

    if not brokers:
        return "No brokers match the given filters."

    lines = [f"{len(brokers)} broker(s):"]
    for b in brokers:
        channels = "/".join(ch.type for ch in b.opt_out)
        flag = " [DISABLED]" if b.disabled else ""
        juris = ",".join(b.jurisdictions)
        lines.append(
            f"  {b.id:<28} {b.priority.value:<6} {b.category.value:<18} "
            f"{juris:<12} {channels}{flag}"
        )
    return "\n".join(lines)


def handle_brokers_show(broker_id: str, output_format: str = "text") -> str:
    """Show full details of one broker by id."""
    try:
        broker = load_broker(broker_id)
    except FileNotFoundError:
        # Fall back to disabled brokers too — `show` is informational.
        for b in load_all_brokers(include_disabled=True):
            if b.id == broker_id:
                broker = b
                break
        else:
            typer.echo(f"Broker '{broker_id}' not found in registry.", err=True)
            raise typer.Exit(1) from None

    if output_format == "json":
        return json.dumps(
            {
                "schema_version": 1,
                "broker": broker.model_dump(mode="json", exclude_none=True),
            },
            indent=2,
            default=str,
        )

    lines = [
        f"Broker: {broker.name}",
        f"  id:               {broker.id}",
        f"  website:          {broker.website}",
        f"  category:         {broker.category.value}",
        f"  priority:         {broker.priority.value}",
        f"  data_sensitivity: {broker.data_sensitivity}",
        f"  jurisdictions:    {', '.join(broker.jurisdictions)}",
        f"  laws:             {', '.join(law.value for law in broker.laws)}",
        f"  disabled:         {broker.disabled}",
    ]
    for i, channel in enumerate(broker.opt_out, 1):
        lines.append(f"  opt_out[{i}]: {channel.type}")
        if isinstance(channel, EmailOptOut):
            lines.append(f"    endpoint: {channel.endpoint}")
            lines.append(f"    template: {channel.template}")
            lines.append(f"    locale:   {channel.locale}")
            lines.append(f"    expected_response_days: {channel.expected_response_days}")
        elif isinstance(channel, WebFormOptOut):
            lines.append(f"    url:      {channel.url}")
            lines.append(f"    steps:    {len(channel.form_spec.steps)}")
    if broker.verification:
        lines.append(f"  verification.ack_keywords:        {broker.verification.ack_keywords}")
        lines.append(
            f"  verification.rejection_keywords:  {broker.verification.rejection_keywords}"
        )
    if broker.notes:
        lines.append("  notes:")
        for nl in broker.notes.strip().splitlines():
            lines.append(f"    {nl}")
    return "\n".join(lines)
