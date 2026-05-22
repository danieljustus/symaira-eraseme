from __future__ import annotations

import json
import time
from datetime import datetime

from openeraseme.core.consent import (
    consume_token,
    issue_token,
    revoke_token,
)
from openeraseme.core.consent import (
    list_tokens as _list_tokens,
)


def handle_grant(
    command: str = "execute",
    ttl: int = 86400,
    revoke: str | None = None,
    revoke_all: bool = False,
    list_tokens: bool = False,
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
        if revoke_token(revoke):
            return f"Token revoked: {revoke}"
        import typer

        typer.echo(
            f"Token not found: {revoke}. "
            "Run 'openeraseme grant --list' to see active tokens.",
            err=True,
        )
        raise typer.Exit(1)

    if revoke_all:
        tokens = _list_tokens()
        if not tokens:
            return "No active tokens to revoke."
        for t in tokens:
            consume_token(t["token"])
        return f"Revoked {len(tokens)} token(s)."

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
        f"Use: OPENERASEME_CONSENT={token} openeraseme {command} ...",
        f"Or:  openeraseme {command} ... --consent {token}",
    ]
    return "\n".join(lines)
