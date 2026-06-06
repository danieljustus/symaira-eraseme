from __future__ import annotations

import time
from datetime import datetime

from symeraseme.core.consent import (
    consume_token,
    issue_token,
    revoke_token,
)
from symeraseme.core.consent import (
    list_tokens as _list_tokens,
)
from symeraseme.core.result_types import CliResult


def handle_grant(
    command: str = "execute",
    ttl: int = 86400,
    revoke: str | None = None,
    revoke_all: bool = False,
    list_tokens: bool = False,
    dry_run: bool = False,
) -> CliResult:
    if list_tokens:
        tokens = _list_tokens()
        if not tokens:
            return CliResult(success=True, data={"tokens": [], "message": "No active tokens."})

        lines = []
        for t in tokens:
            lines.append(
                f"  {t['token']}  cmd={t['command']}  "
                f"expires={datetime.fromtimestamp(t['expires_at']).isoformat()}"
            )
        return CliResult(
            success=True,
            data={"tokens": tokens, "message": "\n".join(lines)},
        )

    if revoke:
        if dry_run:
            return CliResult(
                success=True,
                data={
                    "revoke": revoke,
                    "dry_run": True,
                    "message": f"[DRY RUN] Would revoke token: {revoke}",
                },
            )
        if revoke_token(revoke):
            return CliResult(
                success=True,
                data={"revoke": revoke, "message": f"Token revoked: {revoke}"},
            )
        return CliResult(
            success=False,
            error=(
                f"Token not found: {revoke}. Run 'symeraseme grant --list' to see active tokens."
            ),
        )

    if revoke_all:
        tokens = _list_tokens()
        if dry_run:
            return CliResult(
                success=True,
                data={
                    "revoke_all": True,
                    "token_count": len(tokens),
                    "dry_run": True,
                    "message": f"[DRY RUN] Would revoke {len(tokens)} token(s).",
                },
            )
        if not tokens:
            return CliResult(
                success=True,
                data={"revoke_all": True, "message": "No active tokens to revoke."},
            )
        for t in tokens:
            consume_token(t["token"])
        return CliResult(
            success=True,
            data={
                "revoke_all": True,
                "revoked_count": len(tokens),
                "message": f"Revoked {len(tokens)} token(s).",
            },
        )

    if dry_run:
        expires_at = int(time.time()) + ttl
        result = {
            "command": command,
            "ttl": ttl,
            "expires_at": expires_at,
            "dry_run": True,
        }
        lines = [
            "[DRY RUN] Would issue consent token:",
            f"  Command: {command}",
            f"  TTL: {ttl}s",
            f"  Expires: {datetime.fromtimestamp(expires_at).isoformat()}",
            "",
            f"Use: SYMERASEME_CONSENT=<token> symeraseme {command} ...",
        ]
        result["message"] = "\n".join(lines)
        return CliResult(success=True, data=result)

    token = issue_token(command, ttl=ttl)
    expires_at = int(time.time()) + ttl

    result = {
        "token": token,
        "command": command,
        "ttl": ttl,
        "expires_at": expires_at,
    }
    lines = [
        f"Consent token: {token}",
        f"  Command: {command}",
        f"  TTL: {ttl}s",
        f"  Expires: {datetime.fromtimestamp(expires_at).isoformat()}",
        "",
        f"Use: SYMERASEME_CONSENT={token} symeraseme {command} ...",
        f"Or:  symeraseme {command} ... --consent {token}",
    ]
    result["message"] = "\n".join(lines)
    return CliResult(success=True, data=result)
