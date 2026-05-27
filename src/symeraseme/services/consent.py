from __future__ import annotations

import json
import time
from datetime import datetime

from symeraseme.cli.console import render_error
from symeraseme.core.consent import (
    consume_token,
    issue_token,
    revoke_token,
)
from symeraseme.core.consent import (
    list_tokens as _list_tokens,
)


def handle_grant(
    command: str = "execute",
    ttl: int = 86400,
    revoke: str | None = None,
    revoke_all: bool = False,
    list_tokens: bool = False,
    dry_run: bool = False,
    output_format: str = "text",
) -> str:
    if list_tokens:
        tokens = _list_tokens()
        if not tokens:
            return "No active tokens."

        if output_format == "json":
            return json.dumps(tokens, indent=2, default=str)

        lines = []
        for t in tokens:
            lines.append(
                f"  {t['token']}  cmd={t['command']}  "
                f"expires={datetime.fromtimestamp(t['expires_at']).isoformat()}"
            )
        return "\n".join(lines)

    if revoke:
        if dry_run:
            if output_format == "json":
                return json.dumps(
                    {"revoke": revoke, "dry_run": True},
                    indent=2,
                )
            return f"[DRY RUN] Would revoke token: {revoke}"
        if revoke_token(revoke):
            return f"Token revoked: {revoke}"
        render_error(
            f"Token not found: {revoke}. Run 'symeraseme grant --list' to see active tokens."
        )

    if revoke_all:
        if dry_run:
            tokens = _list_tokens()
            if output_format == "json":
                return json.dumps(
                    {
                        "revoke_all": True,
                        "token_count": len(tokens),
                        "dry_run": True,
                    },
                    indent=2,
                )
            return f"[DRY RUN] Would revoke {len(tokens)} token(s)."
        tokens = _list_tokens()
        if not tokens:
            return "No active tokens to revoke."
        for t in tokens:
            consume_token(t["token"])
        return f"Revoked {len(tokens)} token(s)."

    if dry_run:
        expires_at = int(time.time()) + ttl
        if output_format == "json":
            return json.dumps(
                {
                    "command": command,
                    "ttl": ttl,
                    "expires_at": expires_at,
                    "dry_run": True,
                },
                indent=2,
            )
        lines = [
            "[DRY RUN] Would issue consent token:",
            f"  Command: {command}",
            f"  TTL: {ttl}s",
            f"  Expires: {datetime.fromtimestamp(expires_at).isoformat()}",
            "",
            f"Use: SYMERASEME_CONSENT=\u003ctoken\u003e symeraseme {command} ...",
        ]
        return "\n".join(lines)

    token = issue_token(command, ttl=ttl)

    if output_format == "json":
        return json.dumps(
            {
                "token": token,
                "command": command,
                "ttl": ttl,
                "expires_at": int(time.time()) + ttl,
            },
            indent=2,
        )

    lines = [
        f"Consent token: {token}",
        f"  Command: {command}",
        f"  TTL: {ttl}s",
        f"  Expires: {datetime.fromtimestamp(int(time.time()) + ttl).isoformat()}",
        "",
        f"Use: SYMERASEME_CONSENT={token} symeraseme {command} ...",
        f"Or:  symeraseme {command} ... --consent {token}",
    ]
    return "\n".join(lines)
