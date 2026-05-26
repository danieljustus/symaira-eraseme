from __future__ import annotations

import os
from importlib import resources
from pathlib import Path
from typing import Any

from jinja2 import Environment, FileSystemLoader, select_autoescape

from symeraseme.registry.schema import IdentityProfile


def _templates_dir() -> Path:
    env_dir = os.environ.get("SYMERASEME_RESOURCES")
    if env_dir:
        return Path(env_dir) / "registry" / "laws"
    pkg_root = resources.files("symeraseme")
    candidate = Path(str(pkg_root)) / "registry" / "laws"
    if candidate.exists() and any(candidate.iterdir()):
        return candidate
    for parent in Path(str(pkg_root)).parents:
        if (parent / "registry" / "laws").exists():
            return parent / "registry" / "laws"
    msg = "Could not find templates directory (registry/laws)"
    raise FileNotFoundError(msg)


def _create_env(templates_dir: str | Path | None = None) -> Environment:
    search_path = Path(templates_dir) if templates_dir else _templates_dir()
    return Environment(
        loader=FileSystemLoader(str(search_path)),
        autoescape=select_autoescape(
            enabled_extensions=("html", "htm", "xml", "html.j2", "htm.j2", "xml.j2"),
            default_for_string=True,
        ),
        trim_blocks=True,
        lstrip_blocks=True,
    )


def _profile_vars(profile: IdentityProfile) -> dict[str, Any]:
    return {
        "full_name": profile.full_name,
        "name_variants": profile.name_variants,
        "date_of_birth": str(profile.date_of_birth) if profile.date_of_birth else "",
        "addresses": [
            {
                "street": a.street,
                "city": a.city,
                "postal_code": a.postal_code,
                "country": a.country,
            }
            for a in profile.addresses
        ],
        "email_addresses": profile.email_addresses,
        "phone_numbers": profile.phone_numbers,
        "jurisdictions": profile.jurisdictions,
    }


def render_template(
    template_name: str,
    profile: IdentityProfile | None = None,
    *,
    broker_name: str = "",
    broker_website: str = "",
    brokers: list[dict[str, str]] | None = None,
    templates_dir: str | Path | None = None,
    extra_vars: dict[str, Any] | None = None,
) -> str:
    env = _create_env(templates_dir)

    template = env.get_template(template_name)

    vars: dict[str, Any] = {}

    if profile:
        vars.update(_profile_vars(profile))

    vars["broker_name"] = broker_name
    vars["broker_website"] = broker_website
    vars["brokers"] = brokers or []

    if extra_vars:
        vars.update(extra_vars)

    return template.render(**vars)


def list_templates(templates_dir: str | Path | None = None) -> list[str]:
    search_path = Path(templates_dir) if templates_dir else _templates_dir()
    return sorted([p.name for p in search_path.glob("*.md.j2")])
