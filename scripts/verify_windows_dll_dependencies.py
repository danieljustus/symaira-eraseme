#!/usr/bin/env python3
"""Reject Windows release imports outside the OS and dynamic C runtimes."""

from __future__ import annotations

import re
import sys
from pathlib import Path


DLL_LINE = re.compile(r"^\s+([A-Za-z0-9_.+-]+\.dll)\s*$", re.IGNORECASE)
CRT_DLL = re.compile(
    r"^(?:api-ms-win-crt-|ucrtbase\.dll$|vcruntime[^.]*\.dll$|"
    r"msvcp[^.]*\.dll$|msvcr[^.]*\.dll$|libgcc[^.]*\.dll$|libstdc\+\+[^.]*\.dll$)",
    re.IGNORECASE,
)


def verify(output: str, system32: Path) -> None:
    if not system32.is_dir():
        raise ValueError(f"Windows System32 directory is unavailable: {system32}")

    dlls = sorted(
        {match.group(1).lower() for line in output.splitlines() if (match := DLL_LINE.match(line))}
    )
    if not dlls:
        raise ValueError("dumpbin output contains no parsed DLL dependencies")

    crt = [name for name in dlls if CRT_DLL.match(name)]
    if crt:
        raise ValueError(f"dynamic C runtime dependencies are forbidden: {', '.join(crt)}")

    missing = [
        name
        for name in dlls
        if not name.startswith(("api-ms-win-", "ext-ms-win-"))
        and not (system32 / name).is_file()
    ]
    if missing:
        raise ValueError(f"non-system DLL dependencies are forbidden: {', '.join(missing)}")


def main(argv: list[str] | None = None) -> int:
    args = argv if argv is not None else sys.argv[1:]
    if len(args) != 2:
        print(f"usage: {Path(sys.argv[0]).name} DUMPBIN_OUTPUT SYSTEM32", file=sys.stderr)
        return 2
    try:
        verify(Path(args[0]).read_text(encoding="utf-8-sig"), Path(args[1]))
    except (OSError, ValueError) as error:
        print(f"verify-windows-dll-dependencies: {error}", file=sys.stderr)
        return 1
    print("PASS: Windows imports are OS API sets/system DLLs with no dynamic C runtime")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
