#!/usr/bin/env python3
"""Synthetic offline controls for `make release-dry-run`; not native or release proof."""

import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tests"))
sys.dont_write_bytecode = True

from test_stage_rust_release_archives import executable_header  # noqa: E402

TARGETS = ("darwin-amd64", "darwin-arm64", "linux-amd64", "linux-arm64", "windows-amd64", "windows-arm64")
VERSION = "9.8.7-test.1"


class RustReleaseDryRunContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="rust release contract ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo copy's"
        self.root.mkdir()
        shutil.copyfile(ROOT / "Makefile", self.root / "Makefile")
        (self.root / "LICENSE").write_text("synthetic license\n")
        (self.root / "README.md").write_text("synthetic readme\n")
        (self.root / "scripts").mkdir()
        shutil.copyfile(ROOT / "scripts/stage_rust_release_archives.py", self.root / "scripts/stage_rust_release_archives.py")
        (self.root / "tests").mkdir()
        for name in ("verify_release_archives.py", "test_release_archive_verifier.py"):
            shutil.copyfile(ROOT / "tests" / name, self.root / "tests" / name)
        (self.root / "Cargo.toml").write_text(
            f'[workspace]\nmembers = []\n\n[workspace.package]\nversion = "{VERSION}"\n'
        )
        self.binaries = self.root / "supplied binaries's"
        self.binaries.mkdir()
        for os_name, arch in (t.split("-") for t in TARGETS):
            (self.binaries / f"rust-binary-{os_name}-{arch}").write_bytes(executable_header(os_name, arch))
        self.dist = self.root / "release dist's"

    def run_make(self, *arguments, version=None):
        env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
        env.pop("RELEASE_VERSION", None)
        command = ["make", "-f", "Makefile", "release-dry-run", f"PYTHON={sys.executable}",
                   f"RELEASE_BINARIES={self.binaries}", f"RELEASE_DIST={self.dist}", *arguments]
        if version is not None:
            command.append(f"RELEASE_VERSION={version}")
        return subprocess.run(command, cwd=self.root, env=env, text=True, capture_output=True, timeout=120)

    def test_default_version_comes_from_cargo_and_all_six_archives_verify(self):
        result = self.run_make()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("LOCAL packaging proof only", result.stdout)
        self.assertIn(f"PASS: 6 release archives and SHA-256 entries match {VERSION}", result.stdout)
        self.assertEqual(json.loads((self.dist / "metadata.json").read_text()), {"version": VERSION})
        archives = sorted(p.name for p in self.dist.iterdir() if p.name.endswith((".tar.gz", ".zip")))
        self.assertEqual(len(archives), 6)
        checksums = (self.dist / "checksums.txt").read_text().splitlines()
        self.assertEqual(len(checksums), 6)
        for name in archives:
            self.assertIn(name, (self.dist / "checksums.txt").read_text())
        with tarfile.open(self.dist / f"symeraseme_{VERSION}_linux_amd64.tar.gz") as bundle:
            self.assertEqual(set(bundle.getnames()), {"symeraseme", "LICENSE", "README.md"})
        with zipfile.ZipFile(self.dist / f"symeraseme_{VERSION}_windows_arm64.zip") as bundle:
            self.assertEqual(set(bundle.namelist()), {"symeraseme.exe", "LICENSE", "README.md"})

    def test_native_workflow_runs_this_target_and_discovers_its_controls(self):
        workflow = (ROOT / '.github/workflows/rust-prerelease.yml').read_text()
        self.assertIn('      - Makefile\n', workflow)
        self.assertIn('      - tests/test_make_rust_release_contract.py\n', workflow)
        self.assertIn("python3 -m unittest discover -s tests -p 'test_*release*.py'", workflow)
        self.assertIn('make release-dry-run RELEASE_BINARIES="$BINARIES" RELEASE_DIST="$DIST" RELEASE_VERSION="$RELEASE_VERSION"', workflow)

    def test_explicit_release_version_overrides_cargo(self):
        result = self.run_make(version="1.2.3")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads((self.dist / "metadata.json").read_text()), {"version": "1.2.3"})
        self.assertTrue((self.dist / "symeraseme_1.2.3_darwin_arm64.tar.gz").is_file())

    def test_missing_binary_fails_without_creating_output(self):
        (self.binaries / "rust-binary-linux-arm64").unlink()
        result = self.run_make()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.dist.exists())

    def test_missing_binary_directory_fails_without_creating_output(self):
        shutil.rmtree(self.binaries)
        result = self.run_make()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires RELEASE_BINARIES", result.stderr)
        self.assertFalse(self.dist.exists())

    def test_wrong_target_header_is_rejected_without_creating_output(self):
        (self.binaries / "rust-binary-linux-amd64").write_bytes(executable_header("windows", "amd64"))
        result = self.run_make()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.dist.exists())

    def test_nonempty_destination_is_rejected_and_sentinel_is_preserved(self):
        self.dist.mkdir()
        sentinel = self.dist / "user-output.txt"
        sentinel.write_text("keep me\n")
        result = self.run_make()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("staging directory must be empty", result.stderr)
        self.assertEqual(sentinel.read_text(), "keep me\n")
        self.assertEqual({p.name for p in self.dist.iterdir()}, {"user-output.txt"})

    def test_invalid_release_version_is_rejected(self):
        result = self.run_make(version="../escape")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.dist.exists())


if __name__ == "__main__":
    unittest.main()
