#!/usr/bin/env python3
"""Check the offline Homebrew URL, archive, install, and version contract."""

import re
import sys
from pathlib import Path


FORMULA_URL = re.compile(
    r"https://github\.com/danieljustus/symaira-eraseme/releases/download/"
    r"v(?P<version>[A-Za-z0-9.+-]+)/symeraseme_"
    r"(?P<archive_version>[A-Za-z0-9.+-]+)_"
    r"(?P<os>darwin|linux)_(?P<arch>amd64|arm64)\.tar\.gz"
)
SHA256 = re.compile(r"[0-9a-f]{64}")


def fail(message: str) -> None:
    raise SystemExit(f"Homebrew formula contract: {message}")


def main(formula_path: Path) -> None:
    source = formula_path.read_text(encoding="utf-8")
    urls = {}
    versions = set()
    lines = source.splitlines()
    for index, line in enumerate(lines):
        url_match = re.search(r'^\s*url "([^"]+)"\s*$', line)
        if not url_match:
            continue
        match = FORMULA_URL.fullmatch(url_match.group(1))
        if not match:
            fail(f"unexpected release URL: {url_match.group(1)}")
        sha_line = lines[index + 1] if index + 1 < len(lines) else ""
        sha_match = re.fullmatch(r'\s*sha256 "([^"]+)"\s*', sha_line)
        if not sha_match or not SHA256.fullmatch(sha_match.group(1)):
            fail(f"URL has no valid following SHA-256: {url_match.group(1)}")
        details = match.groupdict()
        if details["version"] != details["archive_version"]:
            fail(f"release and archive versions differ in {url_match.group(1)}")
        versions.add(details["version"])
        key = (details["os"], details["arch"])
        if key in urls:
            fail(f"duplicate release target: {key[0]}-{key[1]}")
        urls[key] = url_match.group(1)

    expected = {(os_name, arch) for os_name in ("darwin", "linux") for arch in ("amd64", "arm64")}
    if set(urls) != expected:
        fail(f"expected four macOS/Linux architecture URLs, got {sorted(urls)}")
    if len(versions) != 1:
        fail(f"formula mixes release versions: {sorted(versions)}")
    if not re.search(r'(?ms)^  def install\n.*?^    bin\.install "symeraseme"\n.*?^  end$', source):
        fail('install must place the archive root binary with bin.install "symeraseme"')
    if not re.search(r'(?ms)^  test do\n.*?^    system "#\{bin\}/symeraseme", "version"\n.*?^  end$', source):
        fail('test block must run the installed symeraseme version command')

    print(f"PASS: {formula_path} has four versioned release URLs, SHA-256 values, install, and version test")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        fail(f"usage: {Path(sys.argv[0]).name} /path/to/Formula/symeraseme.rb")
    main(Path(sys.argv[1]))
