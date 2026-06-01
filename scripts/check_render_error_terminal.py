#!/usr/bin/env python3
"""Verify that every render_error() call is terminal in its block."""

from __future__ import annotations

import ast
import sys
from pathlib import Path


def _is_terminal_call(node: ast.AST) -> bool:
    """Return True if node is a return or raise statement."""
    return isinstance(node, (ast.Return, ast.Raise))


def _find_parent_body(tree: ast.AST, target: ast.AST) -> list[ast.AST] | None:
    """Find the list that contains *target* as a statement."""
    for node in ast.walk(tree):
        for attr in ("body", "orelse", "finalbody"):
            body = getattr(node, attr, None)
            if isinstance(body, list) and target in body:
                return body
    return None


def check_file(filepath: Path) -> list[tuple[int, int]]:
    """Return (render_error_line, next_line) for every non-terminal call."""
    text = filepath.read_text()
    try:
        tree = ast.parse(text)
    except SyntaxError as exc:
        print(f"{filepath}: syntax error {exc}")
        return []

    violations: list[tuple[int, int]] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Expr):
            continue
        call = node.value
        if not isinstance(call, ast.Call):
            continue
        func = call.func
        if not isinstance(func, ast.Name) or func.id != "render_error":
            continue

        body = _find_parent_body(tree, node)
        if body is None:
            continue

        idx = body.index(node)
        if idx < len(body) - 1:
            nxt = body[idx + 1]
            if not _is_terminal_call(nxt):
                violations.append((node.lineno, nxt.lineno))
    return violations


def main() -> int:
    root = Path("src/symeraseme/services")
    if not root.exists():
        print(f"Directory not found: {root}")
        return 1

    exit_code = 0
    for py_file in sorted(root.rglob("*.py")):
        violations = check_file(py_file)
        if violations:
            exit_code = 1
            for render_line, next_line in violations:
                print(
                    f"{py_file}:{render_line}: render_error() "
                    f"followed by executable code at line {next_line}"
                )

    if exit_code == 0:
        print("OK: All render_error() calls are terminal in their blocks.")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
