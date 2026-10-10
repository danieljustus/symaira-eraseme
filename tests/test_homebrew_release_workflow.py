"""Executable guards for Homebrew release publication."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from typing import Optional
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/publish-homebrew.yml"


class HomebrewReleaseWorkflow(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text()
        marker = "      - name: Validate release and publisher credential\n"
        if cls.workflow.count(marker) != 1:
            raise AssertionError("expected exactly one Homebrew release validation step")
        start = cls.workflow.index(marker)
        end = cls.workflow.find("\n      - name:", start + len(marker))
        cls.validation_step = cls.workflow[start:end if end != -1 else len(cls.workflow)]
        if "        run: |\n" not in cls.validation_step:
            raise AssertionError("validation step has no literal shell script")
        shell = cls.validation_step.split("        run: |\n", 1)[1]
        lines = []
        for line in shell.splitlines():
            if line and not line.startswith("          "):
                raise AssertionError(f"unexpected shell indentation in workflow: {line!r}")
            lines.append(line[10:] if line else "")
        cls.script = "\n".join(lines) + "\n"

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.calls = self.root / "gh-calls.json"
        self.fixture = self.root / "release.json"
        self.gh = self.bin / "gh"
        self.gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "Path(os.environ['GH_CALLS']).write_text(json.dumps({"
            "'argv': sys.argv[1:], 'token': os.environ.get('GH_TOKEN')}))\n"
            "expected = ['api', '--method', 'GET', "
            "'repos/danieljustus/symaira-eraseme/releases/tags/' + os.environ['RELEASE_TAG']]\n"
            "if sys.argv[1:] != expected: raise SystemExit(91)\n"
            "if os.environ['GH_EXIT'] != '0': raise SystemExit(92)\n"
            "sys.stdout.write(Path(os.environ['GH_FIXTURE']).read_text())\n"
        )
        self.gh.chmod(0o755)

    @staticmethod
    def release(tag="v0.12.1", *, draft=False, prerelease=False, immutable=True):
        return {
            "tag_name": tag,
            "draft": draft,
            "prerelease": prerelease,
            "immutable": immutable,
        }

    def run_guard(self, *, dry_run="false", payload=None, payload_text=None,
                  api_exit=0, read_token: Optional[str] = "fixture-read-only-token",
                  publisher_token: Optional[str] = "fixture-publisher-token", tag="v0.12.1"):
        if payload_text is None:
            payload_text = json.dumps(payload if payload is not None else self.release(tag))
        self.fixture.write_text(payload_text)
        env = {
            "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
            "HOME": str(self.root),
            "TMPDIR": os.environ.get("TMPDIR", str(self.root)),
            "RELEASE_TAG": tag,
            "DRY_RUN": dry_run,
            "GITHUB_REPOSITORY": "danieljustus/symaira-eraseme",
            "GH_FIXTURE": str(self.fixture),
            "GH_CALLS": str(self.calls),
            "GH_EXIT": str(api_exit),
        }
        if read_token is not None:
            env["GH_TOKEN"] = read_token
        if publisher_token is not None:
            env["HOMEBREW_TAP_GITHUB_TOKEN"] = publisher_token
        return subprocess.run(
            ["bash", "-c", self.script],
            env=env,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=5,
        )

    def assert_api_get(self, *, token="fixture-read-only-token"):
        self.assertTrue(self.calls.exists(), "release lookup did not use the fixture gh client")
        call = json.loads(self.calls.read_text())
        self.assertEqual(
            call["argv"],
            ["api", "--method", "GET",
             "repos/danieljustus/symaira-eraseme/releases/tags/v0.12.1"],
        )
        self.assertEqual(call["token"], token)

    def test_non_dry_run_executes_actual_guard_against_release_fixtures(self):
        cases = [
            ("stable-immutable", self.release(), 0, "v0.12.1", True),
            ("prerelease", self.release(prerelease=True), 0, "v0.12.1", False),
            ("draft", self.release(draft=True), 0, "v0.12.1", False),
            ("mutable", self.release(immutable=False), 0, "v0.12.1", False),
            ("wrong-tag", self.release(tag="v9.9.9"), 0, "v0.12.1", False),
            ("missing-state", {"tag_name": "v0.12.1", "draft": False,
                                "prerelease": False}, 0, "v0.12.1", False),
            ("malformed-json", None, 0, "v0.12.1", False),
            ("api-error", self.release(), 1, "v0.12.1", False),
        ]
        for name, payload, api_exit, tag, accepted in cases:
            with self.subTest(case=name):
                self.calls.unlink(missing_ok=True)
                result = self.run_guard(
                    payload=payload,
                    payload_text="not-json" if name == "malformed-json" else None,
                    api_exit=api_exit,
                    tag=tag,
                )
                self.assertEqual(result.returncode == 0, accepted,
                                 result.stderr or result.stdout)
                self.assert_api_get()

    def test_non_dry_run_fails_closed_without_read_or_publisher_credential(self):
        no_read_token = self.run_guard(read_token=None)
        self.assertNotEqual(no_read_token.returncode, 0)
        self.assertFalse(self.calls.exists(), "guard attempted an unauthenticated lookup")

        no_publisher_token = self.run_guard(publisher_token=None)
        self.assertNotEqual(no_publisher_token.returncode, 0)
        self.assert_api_get()

    def test_dry_run_allows_prerelease_without_tap_credential_or_api_call(self):
        prerelease_fixture = self.release(tag="v0.13.1", prerelease=True)
        result = self.run_guard(
            dry_run="true",
            payload=prerelease_fixture,
            tag="v0.13.1",
            read_token=None,
            publisher_token=None,
        )
        self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
        self.assertFalse(self.calls.exists(), "dry-run performed a release API lookup")

        checkout_marker = "      - name: Checkout Homebrew tap\n"
        checkout_start = self.workflow.index(checkout_marker)
        checkout_end = self.workflow.find("\n      - name:", checkout_start + len(checkout_marker))
        checkout_step = self.workflow[checkout_start:checkout_end]
        self.assertIn("token: ${{ inputs.dry_run && github.token || secrets.HOMEBREW_TAP_GITHUB_TOKEN }}",
                      checkout_step)

        publish_marker = "      - name: Publish updated Formula\n"
        publish_start = self.workflow.index(publish_marker)
        publish_end = self.workflow.find("\n      - name:", publish_start + len(publish_marker))
        publish_step = self.workflow[publish_start:publish_end]
        self.assertIn("if: ${{ !inputs.dry_run }}", publish_step)

        download_marker = "      - name: Download and verify CLI archives\n"
        download_start = self.workflow.index(download_marker)
        download_end = self.workflow.find("\n      - name:", download_start + len(download_marker))
        download_step = self.workflow[download_start:download_end]
        self.assertIn("releases/download/${RELEASE_TAG}", download_step)

    def test_release_state_guard_runs_before_any_tap_checkout(self):
        checkout_index = self.workflow.index("      - name: Checkout Homebrew tap\n")
        validate_index = self.workflow.index("      - name: Validate release and publisher credential\n")
        self.assertLess(validate_index, checkout_index)
        self.assertIn("GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}", self.validation_step)
        self.assertIn("permissions:\n  contents: read", self.workflow)
        for field in (".tag_name == $expected_tag", ".draft == false",
                      ".prerelease == false", ".immutable == true"):
            self.assertIn(field, self.script)


if __name__ == "__main__":
    unittest.main()
