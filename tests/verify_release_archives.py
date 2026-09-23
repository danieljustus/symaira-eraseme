#!/usr/bin/env python3
"""Verify the current GoReleaser archive and checksum contract."""

import hashlib
import json
import re
import sys
import tarfile
import zipfile
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(message)


def main(dist: Path) -> None:
    metadata = json.loads((dist / "metadata.json").read_text())
    version = metadata.get("version")
    if not isinstance(version, str) or not version:
        fail("metadata.json has no release version")

    expected = {
        f"symeraseme_{version}_{os}_{arch}.{extension}"
        for os in ("darwin", "linux")
        for arch in ("amd64", "arm64")
        for extension in ("tar.gz",)
    }
    expected.update(
        f"symeraseme_{version}_windows_{arch}.zip" for arch in ("amd64", "arm64")
    )
    archives = {path.name: path for path in dist.iterdir() if path.name.endswith((".tar.gz", ".zip"))}
    if set(archives) != expected:
        fail(f"archive set mismatch: expected {sorted(expected)}, got {sorted(archives)}")

    checksums: dict[str, str] = {}
    for line in (dist / "checksums.txt").read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (\*?[^/\\]+)", line)
        if not match:
            fail(f"invalid SHA-256 checksum line: {line!r}")
        digest, name = match.groups()
        name = name.removeprefix("*")
        if name in checksums:
            fail(f"duplicate checksum entry: {name}")
        checksums[name] = digest

    if set(checksums) != expected:
        fail(f"checksum set mismatch: expected {sorted(expected)}, got {sorted(checksums)}")

    for name, archive in archives.items():
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if checksums[name] != digest:
            fail(f"SHA-256 mismatch for {name}")

        binary = "symeraseme.exe" if "_windows_" in name else "symeraseme"
        if name.endswith(".tar.gz"):
            with tarfile.open(archive, "r:gz") as bundle:
                members = bundle.getmembers()
                paths = {member.name for member in members}
                top_level = {path.split("/", 1)[0] for path in paths}
                executable = next((item for item in members if item.name == binary), None)
                if executable is None or not executable.isfile() or not executable.mode & 0o111:
                    fail(f"{name} has no executable {binary} at its root")
        else:
            with zipfile.ZipFile(archive) as bundle:
                paths = set(bundle.namelist())
                top_level = {path.split("/", 1)[0] for path in paths}
                if binary not in paths:
                    fail(f"{name} has no {binary} at its root")

        if paths != {binary, "LICENSE", "README.md"} or top_level != paths:
            fail(f"{name} has unexpected root contents: {sorted(paths)}")

    print(f"PASS: {len(archives)} release archives and SHA-256 entries match {version}")


if __name__ == "__main__":
    main(Path(sys.argv[1]) if len(sys.argv) > 1 else Path("dist"))
