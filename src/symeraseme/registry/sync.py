"""Registry sync — pull the latest broker definitions without reinstalling.

For source checkouts this is a thin wrapper around ``git pull --ff-only``
on the registry-containing repo. For pip-installed packages the registry
is bundled with the wheel, so sync explains how to upgrade.

The optional ``--verify-signatures`` flag is documented in the architecture
plan; it is a no-op in v0.1 (full GPG-based supply-chain verification is
scheduled for v0.2) but accepted now so scripts and Agents can adopt the
final shape immediately.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

from symeraseme.cli.types import CliResult
from symeraseme.registry.loader import _registry_dir


def _find_git_root(path: Path) -> Path | None:
    """Walk up the tree from ``path`` looking for a .git directory."""
    current = path.resolve()
    for candidate in (current, *current.parents):
        if (candidate / ".git").exists():
            return candidate
    return None


def _is_detached_head(repo_root: Path) -> bool:
    result = subprocess.run(
        ["git", "symbolic-ref", "--quiet", "HEAD"],
        cwd=str(repo_root),
        capture_output=True,
    )
    # git symbolic-ref returns 1 for detached HEAD, 0 when on a named branch
    return result.returncode != 0


def _run_git_pull(repo_root: Path) -> dict[str, Any]:
    if _is_detached_head(repo_root):
        return {
            "ok": True,
            "detached_head": True,
            "stdout": "",
            "stderr": "",
            "message": "Detached HEAD — no branch to update. Skipping pull.",
        }

    try:
        result = subprocess.run(
            ["git", "pull", "--ff-only"],
            cwd=str(repo_root),
            capture_output=True,
            text=True,
            timeout=60,
        )
    except FileNotFoundError:
        return {
            "ok": False,
            "error": "git executable not found on PATH",
            "stdout": "",
            "stderr": "",
        }
    except subprocess.TimeoutExpired:
        return {
            "ok": False,
            "error": "git pull timed out after 60s",
            "stdout": "",
            "stderr": "",
        }

    return {
        "ok": result.returncode == 0,
        "exit_code": result.returncode,
        "stdout": (result.stdout or "").strip(),
        "stderr": (result.stderr or "").strip(),
    }


def sync_registry(verify_signatures: bool = False) -> dict[str, Any]:
    """Pull the latest broker definitions.

    Returns a structured result with at minimum:
      - ``mode``: "git" | "pip"
      - ``ok``: bool
      - ``message``: human summary
      - ``signature_verification``: "skipped" (v0.1 no-op) | future modes
    """
    registry_dir = _registry_dir()
    git_root = _find_git_root(registry_dir)

    signature_verification = "skipped"
    if verify_signatures:
        # v0.2 will GPG-verify HEAD against MAINTAINERS.toml. For now we
        # accept the flag but make explicit that nothing is actually checked.
        signature_verification = "skipped:not-implemented-in-v0.1"

    if git_root:
        pull = _run_git_pull(git_root)
        return {
            "schema_version": 1,
            "mode": "git",
            "ok": pull["ok"],
            "git_root": str(git_root),
            "registry_dir": str(registry_dir),
            "stdout": pull["stdout"],
            "stderr": pull["stderr"],
            "signature_verification": signature_verification,
            "message": (
                "Registry pulled (git fast-forward)."
                if pull["ok"]
                else (pull.get("error") or pull["stderr"] or "git pull failed")
            ),
        }

    return {
        "schema_version": 1,
        "mode": "pip",
        "ok": True,
        "git_root": None,
        "registry_dir": str(registry_dir),
        "signature_verification": signature_verification,
        "message": (
            "Registry is bundled with the installed package. "
            "To update, upgrade the package: "
            "`pip install --upgrade symeraseme` (or `uv pip install --upgrade symeraseme`)."
        ),
    }


def handle_registry_sync(
    verify_signatures: bool = False,
    output_format: str = "text",
) -> CliResult:
    """CLI handler around :func:`sync_registry`. Invalidates the broker cache
    on success so subsequent CLI calls see the new data without restart.
    """
    result = sync_registry(verify_signatures=verify_signatures)

    if result["ok"]:
        # Invalidate the in-process broker cache so the next call re-reads disk.
        from symeraseme.registry import loader as _loader

        _loader._BROKER_CACHE.clear()

    lines = [f"Registry sync — mode: {result['mode']}"]
    lines.append(f"  ok: {result['ok']}")
    lines.append(f"  registry_dir: {result['registry_dir']}")
    if result.get("git_root"):
        lines.append(f"  git_root: {result['git_root']}")
    if result.get("stdout"):
        lines.append("  git stdout:")
        for line in result["stdout"].splitlines():
            lines.append(f"    {line}")
    if result.get("stderr"):
        lines.append("  git stderr:")
        for line in result["stderr"].splitlines():
            lines.append(f"    {line}")
    if result.get("signature_verification"):
        lines.append(f"  signature_verification: {result['signature_verification']}")
    lines.append(f"  {result['message']}")
    result["message"] = "\n".join(lines)

    return CliResult(
        success=result["ok"],
        data=result,
        error=None if result["ok"] else result.get("message", "Registry sync failed"),
    )
