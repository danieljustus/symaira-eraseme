"""Negative controls for immutable scanner exceptions; no credentials or network."""
import contextlib
import io
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from scripts import check_frozen_secret_scan_scope as scope  # noqa: E402


class FrozenSecretScanScope(unittest.TestCase):
    def test_exact_bytes_complete_history_and_narrow_paths(self):
        with tempfile.TemporaryDirectory(prefix="frozen-scan-scope-") as directory:
            root = Path(directory) / "repo"
            root.mkdir()

            def git(*arguments):
                return subprocess.check_output(["git", "-C", str(root), *arguments],
                                               stderr=subprocess.PIPE, timeout=30)

            git("init", "-q", "-b", "main")
            git("config", "user.name", "Scanner scope regression")
            git("config", "user.email", "88160656+danieljustus@users.noreply.github.com")
            filters = root / ".trufflehog-exclude-paths"
            shutil.copyfile(ROOT / filters.name, filters)
            for name in scope.FIXTURES:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, path)
            git("add", ".")
            git("commit", "-qm", "Reviewed immutable fixture bytes")
            with contextlib.redirect_stdout(io.StringIO()):
                scope.check(root)

            original_filters = filters.read_bytes()
            for extra in (b"^tests/fixtures/.*$\n", (scope.LEGACY + "\n").encode()):
                filters.write_bytes(original_filters + extra)
                with self.assertRaisesRegex(ValueError, "only the five exact"):
                    scope.check(root)
            filters.write_bytes(original_filters)

            for name in scope.FIXTURES:
                path = root / name
                original = path.read_bytes()
                path.write_bytes(original + b"\nchanged\n")
                with self.assertRaisesRegex(ValueError, "bytes changed or missing"):
                    scope.check(root)
                path.unlink()
                with self.assertRaisesRegex(ValueError, "bytes changed or missing"):
                    scope.check(root)
                path.write_bytes(original)

            shallow = Path(directory) / "shallow"
            subprocess.run(["git", "clone", "-q", "--depth=1", root.as_uri(), str(shallow)],
                           check=True, capture_output=True, timeout=30)
            with self.assertRaisesRegex(ValueError, "complete Git history"):
                scope.check(shallow)

            # A reverted finding on a merged side branch must not disappear from scope.
            git("checkout", "-qb", "temporary-change")
            path = root / next(iter(scope.FIXTURES))
            original = path.read_bytes()
            path.write_bytes(original + b"\nchanged historical bytes\n")
            git("add", ".")
            git("commit", "-qm", "Temporary altered fixture")
            path.write_bytes(original)
            git("add", ".")
            git("commit", "-qm", "Restore reviewed fixture")
            git("checkout", "-q", "main")
            git("merge", "--no-ff", "-qm", "Retain side-branch history", "temporary-change")
            with self.assertRaisesRegex(ValueError, "changed in history"):
                scope.check(root)


if __name__ == "__main__":
    unittest.main()
