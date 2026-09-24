#!/usr/bin/env python3
"""Stage local Rust binaries in the current six-archive Go release shape.

This does not build, sign, publish, or inspect binary architecture. It packages
six caller-supplied files and checks the resulting archive contract locally.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import re
import shutil
import stat
import sys
import tarfile
import zipfile
from pathlib import Path


TARGETS = (
    ("darwin", "amd64", "tar.gz", "symeraseme"),
    ("darwin", "arm64", "tar.gz", "symeraseme"),
    ("linux", "amd64", "tar.gz", "symeraseme"),
    ("linux", "arm64", "tar.gz", "symeraseme"),
    ("windows", "amd64", "zip", "symeraseme.exe"),
    ("windows", "arm64", "zip", "symeraseme.exe"),
)


def _regular_input(path: Path, label: str) -> Path:
    try:
        mode = path.lstat().st_mode
    except OSError as error:
        raise ValueError(f"{label}: {error}") from error
    if not stat.S_ISREG(mode):
        raise ValueError(f"{label} must be a regular file: {path}")
    return path


def _add_tar_file(archive: tarfile.TarFile, source: Path, name: str, mode: int) -> None:
    info = tarfile.TarInfo(name)
    info.size = source.stat().st_size
    info.mode = mode
    info.mtime = 0
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    with source.open("rb") as contents:
        archive.addfile(info, contents)


def _write_tar_gz(path: Path, entries: tuple[tuple[Path, str, int], ...]) -> None:
    with path.open("wb") as output:
        with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for source, name, mode in entries:
                    _add_tar_file(archive, source, name, mode)


def _write_zip(path: Path, entries: tuple[tuple[Path, str, int], ...]) -> None:
    with zipfile.ZipFile(path, mode="w", compression=zipfile.ZIP_DEFLATED) as archive:
        for source, name, mode in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | mode) << 16
            with source.open("rb") as contents, archive.open(info, mode="w") as member:
                shutil.copyfileobj(contents, member)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def stage(
    version: str,
    output: Path,
    binaries: dict[tuple[str, str], Path],
    license_path: Path,
    readme_path: Path,
) -> None:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.+-]*", version):
        raise ValueError("version must be a non-empty filename-safe release version")
    if set(binaries) != {(os_name, arch) for os_name, arch, _, _ in TARGETS}:
        raise ValueError("exactly the six darwin/linux/windows amd64/arm64 binaries are required")

    inputs = {
        target: _regular_input(path, f"{target[0]}-{target[1]} binary")
        for target, path in binaries.items()
    }
    license_path = _regular_input(license_path, "LICENSE")
    readme_path = _regular_input(readme_path, "README.md")
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError(f"staging directory must be empty: {output}")

    archive_paths: list[Path] = []
    for os_name, arch, extension, binary_name in TARGETS:
        archive_path = output / f"symeraseme_{version}_{os_name}_{arch}.{extension}"
        entries = (
            (inputs[(os_name, arch)], binary_name, 0o755 if extension == "tar.gz" else 0o755),
            (license_path, "LICENSE", 0o644),
            (readme_path, "README.md", 0o644),
        )
        if extension == "tar.gz":
            _write_tar_gz(archive_path, entries)
        else:
            _write_zip(archive_path, entries)
        archive_paths.append(archive_path)

    (output / "metadata.json").write_text(
        json.dumps({"version": version}, indent=2) + "\n", encoding="utf-8"
    )
    checksum_lines = [f"{_sha256(path)}  {path.name}" for path in sorted(archive_paths)]
    (output / "checksums.txt").write_text(
        "\n".join(checksum_lines) + "\n", encoding="ascii"
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--license", type=Path, default=Path(__file__).resolve().parents[1] / "LICENSE"
    )
    parser.add_argument(
        "--readme", type=Path, default=Path(__file__).resolve().parents[1] / "README.md"
    )
    for os_name, arch, _, _ in TARGETS:
        parser.add_argument(f"--{os_name}-{arch}", required=True, type=Path)
    args = parser.parse_args(argv)
    binaries = {
        (os_name, arch): getattr(args, f"{os_name}_{arch}")
        for os_name, arch, _, _ in TARGETS
    }
    try:
        stage(args.version, args.output, binaries, args.license, args.readme)
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"stage-rust-release-archives: {error}", file=sys.stderr)
        return 1
    print(f"Staged six local Rust release archives at {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
