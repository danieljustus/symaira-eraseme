#!/usr/bin/env python3
"""Verify the current GoReleaser archive and checksum contract."""

import hashlib
import json
import re
import stat
import sys
import tarfile
import zipfile
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(message)


def main(dist: Path, require_go_fallback: bool = False) -> None:
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
        fallback = "symeraseme-go.exe" if "_windows_" in name else "symeraseme-go"
        expected_members = {binary, "LICENSE", "README.md"}
        if require_go_fallback:
            expected_members.add(fallback)
        if name.endswith(".tar.gz"):
            with tarfile.open(archive, "r:gz") as bundle:
                members = bundle.getmembers()
                names = [member.name for member in members]
                if len(names) != len(set(names)):
                    fail(f"{name} has duplicate archive members")
                paths = set(names)
                top_level = {path.split("/", 1)[0] for path in paths}
                by_name = {member.name: member for member in members}
                if any(path not in by_name or not by_name[path].isfile() for path in expected_members):
                    fail(f"{name} expected members must be regular files")
                for executable in (binary, fallback) if require_go_fallback else (binary,):
                    if not by_name[executable].mode & 0o111:
                        fail(f"{name} has no executable {executable} at its root")
        else:
            with zipfile.ZipFile(archive) as bundle:
                infos = bundle.infolist()
                names = [info.filename for info in infos]
                if len(names) != len(set(names)):
                    fail(f"{name} has duplicate archive members")
                paths = set(names)
                top_level = {path.split("/", 1)[0] for path in paths}
                by_name = {info.filename: info for info in infos}
                for path in expected_members:
                    info = by_name.get(path)
                    file_type = stat.S_IFMT(info.external_attr >> 16) if info else 0
                    if info is None or info.is_dir() or file_type not in (0, stat.S_IFREG):
                        fail(f"{name} expected members must be regular files")

        if paths != expected_members or top_level != paths:
            fail(f"{name} has unexpected root contents: {sorted(paths)}")

    print(f"PASS: {len(archives)} release archives and SHA-256 entries match {version}")


if __name__ == "__main__":
    args = sys.argv[1:]
    require_go_fallback = "--require-go-fallback" in args
    args = [arg for arg in args if arg != "--require-go-fallback"]
    if len(args) > 1:
        raise SystemExit("usage: verify_release_archives.py [DIST] [--require-go-fallback]")
    main(Path(args[0]) if args else Path("dist"), require_go_fallback)
