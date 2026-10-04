#!/usr/bin/env python3
"""Fetch only the pinned last published Go release for a rollback rehearsal."""
import argparse
import hashlib
import importlib.util
import json
import pathlib
import platform
import stat
import sys
import tarfile
import urllib.request
import zipfile

REPO = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("go_build_info", REPO / "rust-tests/parity/go_build_info.py")
info = importlib.util.module_from_spec(spec)
spec.loader.exec_module(info)


def fetch(directory, native_target):
    document = info.release_manifest()
    records = [r for r in document["native_archives"] if r["native_target"] == native_target]
    assert len(records) == 1
    record = records[0]
    directory = pathlib.Path(directory)
    assert directory.is_absolute() and not directory.is_symlink()
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    directory = directory.resolve()
    assert not directory.is_relative_to(REPO.resolve()), "retained binary must stay outside source"
    base = "https://github.com/danieljustus/symaira-eraseme/releases/download/v0.12.1/"

    def download(name, size, digest):
        assert pathlib.PurePosixPath(name).name == name
        destination = directory / name
        assert not destination.is_symlink()
        if not destination.exists():
            request = urllib.request.Request(base + name, headers={"User-Agent": "symeraseme-rollback"})
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read(size + 1)
            assert len(raw) == size and hashlib.sha256(raw).hexdigest() == digest, "actual published archive changed"
            with destination.open("xb") as stream:
                stream.write(raw)
        assert destination.is_file() and destination.stat().st_size == size
        assert hashlib.sha256(destination.read_bytes()).hexdigest() == digest
        return destination

    checks = document["checksums_asset"]
    checksum_path = download("checksums.txt", checks["bytes"], checks["sha256"])
    checksums = {}
    for line in checksum_path.read_text().splitlines():
        digest, name = line.split()
        assert name not in checksums
        checksums[name] = digest
    assert checksums[record["archive"]] == record["archive_sha256"]
    archive = download(record["archive"], record["archive_bytes"], record["archive_sha256"])
    name = "symeraseme.exe" if native_target.startswith("windows/") else "symeraseme"
    binary = directory / name
    assert not binary.is_symlink()
    if not binary.exists():
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as package:
                members = [m for m in package.infolist() if pathlib.PurePosixPath(m.filename).name == name]
                assert len(members) == 1 and not members[0].is_dir() and not stat.S_ISLNK(members[0].external_attr >> 16)
                assert members[0].file_size == record["binary_bytes"]
                raw = package.read(members[0])
        else:
            with tarfile.open(archive, "r:gz") as package:
                members = [m for m in package.getmembers() if pathlib.PurePosixPath(m.name).name == name]
                assert len(members) == 1 and members[0].isfile() and members[0].size == record["binary_bytes"]
                with package.extractfile(members[0]) as stream:
                    raw = stream.read(record["binary_bytes"] + 1)
        assert len(raw) == record["binary_bytes"] and hashlib.sha256(raw).hexdigest() == record["binary_sha256"]
        with binary.open("xb") as stream:
            stream.write(raw)
        binary.chmod(0o700)
    metadata, provenance = info.verify_release(binary, native_target)
    result = {"binary": str(binary), "native_target": native_target,
              "scope": "actual last published Go archive for rollback rehearsal only",
              "sdk_required": False, "source_revision": metadata["source_revision"], "release": provenance}
    (directory / "retained-release.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    operating_system = {"darwin": "darwin", "linux": "linux", "win32": "windows"}[sys.platform]
    architecture = {"x86_64": "amd64", "AMD64": "amd64", "aarch64": "arm64", "arm64": "arm64", "ARM64": "arm64"}[platform.machine()]
    print(json.dumps(fetch(args.output_dir, operating_system + "/" + architecture)))
