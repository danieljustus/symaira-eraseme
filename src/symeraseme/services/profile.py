"""Profile-related CLI handlers."""

from __future__ import annotations

import json

from symeraseme import __version__
from symeraseme.cli.console import render_error
from symeraseme.core.identity import load_profile, profile_exists, save_profile
from symeraseme.core.templating import render_template as _render
from symeraseme.registry.schema import IdentityProfile


def handle_version(output_format: str = "text") -> str:
    msg = f"Symaira EraseMe v{__version__}"
    if output_format == "json":
        return json.dumps({"version": __version__})
    return msg


def handle_init_profile(full_name: str, email: str) -> str:
    profile = IdentityProfile(full_name=full_name, email_addresses=[email])
    path = save_profile(profile)
    action = "Updated" if profile_exists() else "Created"
    return f"{action} encrypted identity profile at {path}"


def handle_show_profile(output_format: str = "text") -> str:
    if not profile_exists():
        render_error("No identity profile found. Run 'symeraseme init-profile' first.")

    profile = load_profile()
    if output_format == "json":
        return json.dumps(profile.model_dump(), indent=2, default=str)

    lines = [f"Name:  {profile.full_name}"]
    for e in profile.email_addresses:
        lines.append(f"Email: {e}")
    for a in profile.addresses:
        lines.append(f"Address: {a.street}, {a.city}, {a.country}")
    for j in profile.jurisdictions:
        lines.append(f"Jurisdiction: {j}")
    return "\n".join(lines)


def handle_render_template(
    template: str,
    broker_name: str = "",
    broker_website: str = "",
    output_format: str = "text",
) -> str:
    profile = load_profile() if profile_exists() else None
    result = _render(
        template,
        profile=profile,
        broker_name=broker_name,
        broker_website=broker_website,
    )
    if output_format == "json":
        return json.dumps({"template": result}, indent=2)
    return result
