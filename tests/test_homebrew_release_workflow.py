"""Executable guards for Homebrew release publication."""
import hashlib
import json
import os
from pathlib import Path
import re
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
        run_marker = "        run: &verify_release |\n"
        if run_marker not in cls.validation_step:
            raise AssertionError("validation step has no literal shell script")
        shell = cls.validation_step.split(run_marker, 1)[1]
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
        self.assertIn("token: ${{ github.token }}", checkout_step)
        self.assertIn("persist-credentials: false", checkout_step)
        self.assertNotIn("secrets.HOMEBREW_TAP_GITHUB_TOKEN", checkout_step)

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

    def test_renderer_avoids_redundant_version_and_native_checks_audit_it(self):
        # Labelled local byte fixtures exercise the actual renderer, not release assets.
        assets = self.root / "symeraseme-assets"
        assets.mkdir()
        expected = {}
        for target in ("darwin_arm64", "linux_arm64", "linux_amd64"):
            name = f"symeraseme_0.14.1_{target}.tar.gz"
            data = f"labelled renderer fixture: {target}\n".encode()
            (assets / name).write_bytes(data)
            expected[name] = hashlib.sha256(data).hexdigest()
        # shasum is the native macOS equivalent of the runner's sha256sum.
        checksum = self.bin / "sha256sum"
        checksum.write_text('#!/bin/sh\nexec shasum -a 256 "$@"\n')
        checksum.chmod(0o755)
        env = {"PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
               "HOME": str(self.root), "RUNNER_TEMP": str(self.root),
               "TMPDIR": os.environ.get("TMPDIR", str(self.root)),
               "RELEASE_TAG": "v0.14.1", "GITHUB_REPOSITORY": "danieljustus/symaira-eraseme"}
        subprocess.run(["git", "init", "--quiet", "tap"], cwd=self.root, env=env,
                       check=True, capture_output=True, timeout=5)
        render = self.workflow.split("      - name: Render and validate Formula\n", 1)[1]
        render = render.split("      - name: Retain the rendered Formula\n", 1)[0]
        shell = render.split("        run: |\n", 1)[1]
        script = "\n".join(line[10:] if line else "" for line in shell.splitlines())
        result = subprocess.run(["bash", "-c", script], cwd=self.root, env=env,
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        formula = (self.root / "tap/Formula/symeraseme.rb").read_text()
        self.assertNotRegex(formula, r"(?m)^\s*version\s")
        self.assertIn('system bin/"symeraseme", "version"', formula)
        pairs = re.findall(r'url "([^\"]+)"\s+sha256 "([0-9a-f]{64})"', formula)
        base = "https://github.com/danieljustus/symaira-eraseme/releases/download/v0.14.1/"
        self.assertEqual(len(pairs), 3)
        self.assertEqual(set(pairs), {(base + name, digest) for name, digest in expected.items()})
        install = self.workflow.split("  install-check:\n", 1)[1].split("  publish-tap:\n", 1)[0]
        audit = "brew audit --strict --formula local/symeraseme-check/symeraseme"
        self.assertIn(audit, install)
        self.assertLess(install.index(audit), install.index("brew install --formula"))
        self.assertIn("brew info --json=v2 --formula local/symeraseme-check/symeraseme", install)
        self.assertIn(".formulae[0].versions.stable", install)
        self.assertIn('= "$version"', install)

    def test_release_state_guard_runs_before_any_tap_checkout(self):
        checkout_index = self.workflow.index("      - name: Checkout Homebrew tap\n")
        validate_index = self.workflow.index("      - name: Validate release and publisher credential\n")
        self.assertLess(validate_index, checkout_index)
        self.assertIn("GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}", self.validation_step)
        self.assertIn("permissions:\n  contents: read", self.workflow)
        for field in (".tag_name == $expected_tag", ".draft == false",
                      ".prerelease == false", ".immutable == true"):
            self.assertIn(field, self.script)

    def test_protected_tap_publication_follows_native_checks_and_fails_closed(self):
        # Labelled local git/gh fixtures; no actual repository or API writes.
        prepare, publication = self.workflow.split("\n  publish-tap:\n", 1)
        self.assertNotIn("git -C tap push", prepare)
        self.assertNotIn("secrets.HOMEBREW_TAP_GITHUB_TOKEN", prepare.split("    steps:\n", 1)[0])
        self.assertEqual(prepare.count("secrets.HOMEBREW_TAP_GITHUB_TOKEN"), 1)
        self.assertIn("HOMEBREW_TAP_GITHUB_TOKEN: ${{ secrets.HOMEBREW_TAP_GITHUB_TOKEN }}",
                      self.validation_step)
        self.assertNotIn("secrets.HOMEBREW_TAP_GITHUB_TOKEN", publication.split("    steps:\n", 1)[0])
        self.assertIn("    needs: install-check\n", publication)
        self.assertIn("        os: [macos-14, ubuntu-24.04, ubuntu-24.04-arm]", prepare)
        self.assertIn("    if: ${{ !inputs.dry_run }}\n", publication)
        self.assertIn("          DRY_RUN: 'false'\n        run: *verify_release\n", publication)
        self.assertLess(publication.index("run: *verify_release"), publication.index("Checkout tap"))
        step = publication.split("      - name: Publish updated Formula\n", 1)[1]
        self.assertIn("GH_TOKEN: ${{ secrets.HOMEBREW_TAP_GITHUB_TOKEN }}", step)
        shell = step.split("        run: |\n", 1)[1]
        script = "\n".join(line[10:] if line else "" for line in shell.splitlines())
        (self.root / "tap/Formula").mkdir(parents=True)
        (self.root / "formula").mkdir()
        (self.root / "formula/symeraseme.rb").write_text("verified fixture Formula\n")
        calls_path = self.root / "publication-calls.jsonl"
        mock = (
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "tool, args = Path(sys.argv[0]).name, sys.argv[1:]\n"
            "with Path(os.environ['PUBLICATION_CALLS']).open('a') as out:\n"
            "    out.write(json.dumps([tool, args]) + '\\n')\n"
            "case = os.environ['PUBLICATION_CASE']\n"
            "if tool == 'git':\n"
            "    if args[:2] != ['-C', 'tap']: raise SystemExit(91)\n"
            "    args = args[2:]\n"
            "    if args == ['diff', '--quiet', '--', 'Formula/symeraseme.rb']:\n"
            "        raise SystemExit(0 if case == 'unchanged' else 1)\n"
            "    if args == ['rev-parse', 'HEAD']: print('a' * 40)\n"
            "    if args[:2] == ['push', 'origin']:\n"
            "        if args[2] == 'main' or case == 'push-error': raise SystemExit(1)\n"
            "elif tool == 'gh':\n"
            "    if args[:2] == ['pr', 'create']:\n"
            "        if case == 'pr-error': raise SystemExit(1)\n"
            "        print('https://github.com/fixture/tap/pull/17')\n"
            "    elif args[:2] == ['pr', 'merge']:\n"
            "        if case == 'merge-error': raise SystemExit(1)\n"
            "    else: raise SystemExit(92)\n"
            "else: raise SystemExit(93)\n"
        )
        for tool in ("git", "gh"):
            path = self.bin / tool
            path.write_text(mock)
            path.chmod(0o755)
        for case in ("changed", "unchanged", "push-error", "pr-error", "merge-error"):
            with self.subTest(case=case):
                calls_path.write_text("")
                env = {"PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                       "HOME": str(self.root), "RUNNER_TEMP": str(self.root),
                       "RELEASE_TAG": "v0.14.1", "TAP_REPOSITORY": "fixture/tap",
                       "GITHUB_RUN_ID": "4242", "GITHUB_RUN_ATTEMPT": "1",
                       "PUBLICATION_CASE": case, "PUBLICATION_CALLS": str(calls_path)}
                result = subprocess.run(["bash", "-c", script], cwd=self.root, env=env,
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode == 0, case in ("changed", "unchanged"), result.stderr)
                calls = [json.loads(line) for line in calls_path.read_text().splitlines()]
                pushes = [args for tool, args in calls if tool == "git" and args[2:4] == ["push", "origin"]]
                creates = [args for tool, args in calls if tool == "gh" and args[:2] == ["pr", "create"]]
                merges = [args for tool, args in calls if tool == "gh" and args[:2] == ["pr", "merge"]]
                self.assertEqual(pushes, [] if case == "unchanged" else
                                 [["-C", "tap", "push", "origin", "homebrew/symeraseme-v0.14.1-4242-1"]])
                self.assertEqual(len(creates), int(case not in ("unchanged", "push-error")))
                self.assertEqual(len(merges), int(case in ("changed", "merge-error")))
                if creates:
                    self.assertEqual(creates[0][:10], ["pr", "create", "--repo", "fixture/tap", "--base",
                                                     "main", "--head", "homebrew/symeraseme-v0.14.1-4242-1",
                                                     "--title", "Update symeraseme to v0.14.1"])
                if merges:
                    self.assertEqual(merges[0], ["pr", "merge", "https://github.com/fixture/tap/pull/17",
                                                "--repo", "fixture/tap", "--squash", "--auto",
                                                "--match-head-commit", "a" * 40])
                self.assertTrue(all("--admin" not in args and "--force" not in args for _, args in calls))


if __name__ == "__main__":
    unittest.main()
