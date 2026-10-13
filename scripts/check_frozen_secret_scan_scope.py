#!/usr/bin/env python3
"""Bind four scanner exceptions to reviewed bytes across complete reachable history."""
import hashlib
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PREFIX = "tests/fixtures/go-frozen/refresh-8973a584/windows-migration/"
# These privacy-reviewed native records contain SHA-256 provenance values that
# TruffleHog 3.97.5 mistakes for provider keys. Never exempt changed fixture bytes.
FIXTURES = {
    PREFIX + "windows-amd64/command-surface.json": "c999eb8ee1a4ac62b12a3344aa291e3171f0468da2281ed6c1fc12d7153aa891",
    PREFIX + "windows-amd64/filesystem.json": "341d1567786470c45cbbd315614dd76506f38a031b356f41a6ecec8f53a148f9",
    PREFIX + "windows-arm64/command-surface.json": "70f9f06c2b544bca3f1e77e88cc238da3d1ef5d290b98c8fd271d13744dbee81",
    PREFIX + "windows-arm64/filesystem.json": "83b38e1fe6e3fa36e87ce5991abde66b9533ec92d3c9ae7e4e7bd20a097cab72",
}
LEGACY = r"^internal/registry/sync_test\.go$"


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments], timeout=30)


def check(root=ROOT):
    if git(root, "rev-parse", "--is-shallow-repository").strip() != b"false":
        raise ValueError("complete Git history is required for scanner exceptions")
    expressions = [line for line in (root / ".trufflehog-exclude-paths").read_text().splitlines()
                   if line and not line.startswith("#")]
    expected = {LEGACY, *("^" + re.escape(path) + "$" for path in FIXTURES)}
    if len(expressions) != len(expected) or set(expressions) != expected:
        raise ValueError("scanner exclusions must contain only the five exact protected paths")
    checked = set()
    for path, digest in FIXTURES.items():
        current = root / path
        if current.is_symlink() or not current.is_file() or hashlib.sha256(current.read_bytes()).hexdigest() != digest:
            raise ValueError(f"scanner exception bytes changed or missing: {path}")
        revisions = git(root, "rev-list", "--full-history", "HEAD", "--", path).decode().splitlines()
        if not revisions:
            raise ValueError(f"scanner exception has no recorded history: {path}")
        # Full history includes temporary changes later reverted, and merge parents.
        for revision in revisions:
            entries = git(root, "ls-tree", "-z", revision, "--", path).split(b"\0")
            for entry in filter(None, entries):
                metadata, filename = entry.split(b"\t", 1)
                mode, kind, oid = metadata.decode().split()
                if filename.decode() != path or kind != "blob" or mode != "100644":
                    raise ValueError(f"invalid scanner exception tree entry: {path}")
                key = (path, oid)
                if key not in checked:
                    if hashlib.sha256(git(root, "cat-file", "blob", oid)).hexdigest() != digest:
                        raise ValueError(f"scanner exception changed in history: {path} at {revision}")
                    checked.add(key)
    print(f"Verified {len(FIXTURES)} immutable scanner exception paths and {len(checked)} historical blobs")


if __name__ == "__main__":
    check()
