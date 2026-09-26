#!/usr/bin/env python3
"""Small malformed-archive checks for verify_release_archives.py."""

import hashlib
import io
import json
import sys
import tarfile
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
from verify_release_archives import main


def create_dist(root: Path, duplicate_suffix: str = "", symlink_suffix: str = "") -> None:
    version = "0.1.0"
    (root / "metadata.json").write_text(json.dumps({"version": version}))
    names = [
        f"symeraseme_{version}_{os}_{arch}.tar.gz"
        for os in ("darwin", "linux")
        for arch in ("amd64", "arm64")
    ] + [f"symeraseme_{version}_windows_{arch}.zip" for arch in ("amd64", "arm64")]
    lines = []
    for name in names:
        binary = "symeraseme.exe" if name.endswith(".zip") else "symeraseme"
        contents = ((binary, b"first"), ("LICENSE", b"license"), ("README.md", b"readme"))
        archive_path = root / name
        if name.endswith(".tar.gz"):
            with tarfile.open(archive_path, "w:gz") as archive:
                for member_name, data in contents + (((binary, b"duplicate"),) if duplicate_suffix and name.endswith(duplicate_suffix) else ()):
                    member = tarfile.TarInfo(member_name)
                    if member_name == binary and symlink_suffix and name.endswith(symlink_suffix):
                        member.type = tarfile.SYMTYPE
                        member.linkname = "malicious-target"
                        archive.addfile(member)
                        continue
                    member.size = len(data)
                    member.mode = 0o755 if member_name == binary else 0o644
                    archive.addfile(member, io.BytesIO(data))
        else:
            with warnings.catch_warnings(), zipfile.ZipFile(archive_path, "w") as archive:
                warnings.simplefilter("ignore", UserWarning)
                for member_name, data in contents + (((binary, b"duplicate"),) if duplicate_suffix and name.endswith(duplicate_suffix) else ()):
                    info = zipfile.ZipInfo(member_name)
                    mode = 0o120777 if member_name == binary and symlink_suffix and name.endswith(symlink_suffix) else (
                        0o100755 if member_name == binary else 0o100644
                    )
                    info.external_attr = mode << 16
                    archive.writestr(info, data)
        lines.append(f"{hashlib.sha256(archive_path.read_bytes()).hexdigest()}  {name}")
    (root / "checksums.txt").write_text("\n".join(lines) + "\n")


class DuplicateArchiveMemberTests(unittest.TestCase):
    def test_duplicate_tar_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            create_dist(dist, ".tar.gz")
            with self.assertRaisesRegex(SystemExit, "duplicate archive members"):
                main(dist)

    def test_symlink_tar_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            create_dist(dist, symlink_suffix=".tar.gz")
            with self.assertRaisesRegex(SystemExit, "expected members must be regular files"):
                main(dist)

    def test_symlink_zip_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            create_dist(dist, symlink_suffix=".zip")
            with self.assertRaisesRegex(SystemExit, "expected members must be regular files"):
                main(dist)

    def test_duplicate_zip_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            create_dist(dist, ".zip")
            with self.assertRaisesRegex(SystemExit, "duplicate archive members"):
                main(dist)

    def test_dual_backend_mode_rejects_archives_without_go_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            create_dist(dist)
            with self.assertRaisesRegex(SystemExit, "expected members must be regular files"):
                main(dist, require_go_fallback=True)


if __name__ == "__main__":
    unittest.main()
