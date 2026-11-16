"""Standalone evidence recorder: report shape and verifier rules, without real builds."""

import contextlib
import hashlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
sys.dont_write_bytecode = True

import record_standalone_evidence as rse
from record_standalone_evidence import (
    ARTIFACT, BIN_NAME, BUILD_COMMAND, REPORT, CommandResult, main, record,
    resolve_corekit_release, verify_committed,
)

COREKIT_TAG = "v0.17.0"
COREKIT_COMMIT = "0c2c7769ac6074264e1c02fcdf903188b4f3fa08"
FAKE_BINARY = b"\xcf\xfa\xed\xfefake rust binary"


def git(repo: Path, *args: str) -> str:
    env = {
        "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.test",
        "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.test",
        "PATH": os.environ["PATH"], "HOME": str(repo),
    }
    return subprocess.run(
        ["git", *args], cwd=repo, env=env, check=True, text=True, stdout=subprocess.PIPE,
    ).stdout.strip()


class FakeToolchain:
    """Real git, canned cargo/rustc and standalone binary invocations."""

    def __init__(self, *, build_exit: int = 0, version_exit: int = 0) -> None:
        self.calls: list[tuple[tuple[str, ...], Path, dict | None]] = []
        self.build_exit = build_exit
        self.version_exit = version_exit

    def __call__(self, argv, cwd, env=None) -> CommandResult:
        argv = tuple(argv)
        self.calls.append((argv, Path(cwd), None if env is None else dict(env)))
        if argv[0] == "git":
            done = subprocess.run(list(argv), cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            return CommandResult(argv, done.returncode, done.stdout, done.stderr)
        if argv == ("cargo", "--version"):
            return CommandResult(argv, 0, "cargo 1.98.0 (deadbeef 2026-08-01)\n", "")
        if argv == ("rustc", "--version"):
            return CommandResult(argv, 0, "rustc 1.98.0 (deadbeef 2026-08-01)\n", "")
        if argv == ("rustc", "-vV"):
            return CommandResult(argv, 0, "rustc 1.98.0\nhost: aarch64-apple-darwin\nrelease: 1.98.0\n", "")
        if list(argv) == BUILD_COMMAND:
            if self.build_exit == 0:
                out = Path(env["CARGO_TARGET_DIR"]) / "release" / BIN_NAME
                out.parent.mkdir(parents=True, exist_ok=True)
                out.write_bytes(FAKE_BINARY)
            return CommandResult(argv, self.build_exit, "", "   Compiling x\n    Finished `release` profile\n")
        if argv[:2] == ("env", "-i"):
            home = Path(argv[3].removeprefix("HOME="))
            assert home.is_dir() and not any(home.iterdir()), "HOME must be an empty directory"
            assert argv[2] == "PATH=/usr/bin:/bin" and argv[4] == f"./{ARTIFACT}" and argv[5] == "version"
            assert (Path(cwd) / ARTIFACT).read_bytes() == FAKE_BINARY
            assert env is None
            return CommandResult(argv, self.version_exit, "symeraseme version 9.9.9\n", "")
        raise AssertionError(f"unexpected command {argv}")


def make_repo(root: Path) -> tuple[Path, str]:
    repo = root / "repo"
    (repo / "evidence" / "artifact").mkdir(parents=True)
    (repo / "Cargo.toml").write_text('[workspace]\n[workspace.package]\nversion = "9.9.9"\n')
    (repo / "Cargo.lock").write_text("# lock\nversion = 4\n")
    (repo / ".gitignore").write_text("build/\n")
    (repo / REPORT).write_text('{"old": true}\n')
    (repo / ARTIFACT).write_bytes(b"old binary")
    (repo / "evidence" / "rollback.json").write_text("{}\n")
    git(repo, "init", "-q", "-b", "main")
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", "prepare")
    return repo, git(repo, "rev-parse", "HEAD")


class RecordStandaloneEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.repo, self.source = make_repo(self.root)
        self.target = self.root / "target"

    def _record(self, toolchain: FakeToolchain | None = None, **overrides):
        toolchain = toolchain or FakeToolchain()
        params = dict(
            repo=self.repo, tag="v9.9.9", source_commit=self.source,
            corekit_tag=COREKIT_TAG, corekit_commit=COREKIT_COMMIT, target_dir=self.target,
        )
        params.update(overrides)
        return record(**params, runner=toolchain, base_env={"PATH": "/usr/bin"}), toolchain

    def _commit_evidence(self) -> str:
        git(self.repo, "add", REPORT, ARTIFACT)
        git(self.repo, "commit", "-q", "-m", "evidence")
        return git(self.repo, "rev-parse", "HEAD")

    def _verify(self, **overrides):
        params = dict(
            repo=self.repo, tag="v9.9.9", source_commit=self.source,
            corekit_tag=COREKIT_TAG, corekit_commit=COREKIT_COMMIT,
        )
        params.update(overrides)
        return verify_committed(**params, runner=FakeToolchain())

    def test_report_has_exactly_the_fields_the_verifier_reads(self) -> None:
        report, toolchain = self._record()
        on_disk = json.loads((self.repo / REPORT).read_text())
        self.assertEqual(on_disk, report)
        self.assertEqual(list(report), ["tag", "commit", "artifact", "artifact_sha256", "observed", "rust_build"])
        self.assertEqual((report["tag"], report["commit"]), (COREKIT_TAG, COREKIT_COMMIT))
        self.assertEqual(report["artifact"], ARTIFACT)
        self.assertEqual(report["artifact_sha256"], hashlib.sha256(FAKE_BINARY).hexdigest())
        self.assertEqual((self.repo / ARTIFACT).read_bytes(), FAKE_BINARY)
        self.assertTrue(os.access(self.repo / ARTIFACT, os.X_OK))

        observed = report["observed"]
        self.assertEqual(set(observed), {"command", "exit_code", "stdout", "stderr"})
        self.assertTrue(observed["command"].startswith("env -i PATH=/usr/bin:/bin HOME="))
        self.assertTrue(observed["command"].endswith(f" ./{ARTIFACT} version"))
        self.assertEqual((observed["exit_code"], observed["stdout"], observed["stderr"]), (0, "symeraseme version 9.9.9\n", ""))

        build = report["rust_build"]
        self.assertEqual(build["source_commit"], self.source)
        self.assertTrue(build["cargo_version"].startswith("cargo "))
        self.assertTrue(build["rustc_version"].startswith("rustc "))
        self.assertEqual(build["target"], "aarch64-apple-darwin")
        self.assertEqual(build["cargo_lock_sha256"], hashlib.sha256((self.repo / "Cargo.lock").read_bytes()).hexdigest())
        self.assertEqual(build["observed"]["command"], BUILD_COMMAND)
        self.assertEqual(build["observed"]["command"][:2], ["cargo", "build"])
        self.assertIn("--locked", build["observed"]["command"])
        self.assertIn("--release", build["observed"]["command"])
        self.assertEqual(build["observed"]["exit_code"], 0)
        self.assertIn("Finished", build["observed"]["stderr"])

        build_env = next(env for argv, _, env in toolchain.calls if list(argv) == BUILD_COMMAND)
        self.assertEqual(build_env["CARGO_TARGET_DIR"], str(self.target))
        self.assertEqual(build_env["CARGO_TERM_COLOR"], "never")

    def test_rollback_files_are_left_alone(self) -> None:
        self._record()
        self.assertEqual((self.repo / "evidence" / "rollback.json").read_text(), "{}\n")

    def test_committed_evidence_passes_verification(self) -> None:
        self._record()
        self._commit_evidence()
        self.assertEqual(self._verify(), [])

    def test_verification_rejects_other_files_changed_after_source(self) -> None:
        self._record()
        (self.repo / "Cargo.toml").write_text((self.repo / "Cargo.toml").read_text() + "# drift\n")
        git(self.repo, "add", "Cargo.toml")
        self._commit_evidence()
        self.assertTrue(any("Cargo.toml" in problem for problem in self._verify()))

    def test_verification_rejects_lock_change_and_artifact_tamper(self) -> None:
        self._record()
        (self.repo / "Cargo.lock").write_text("# changed\n")
        git(self.repo, "add", "Cargo.lock")
        self._commit_evidence()
        problems = self._verify()
        self.assertTrue(any("cargo_lock_sha256" in problem for problem in problems))

        (self.repo / ARTIFACT).write_bytes(b"tampered")
        git(self.repo, "add", ARTIFACT)
        git(self.repo, "commit", "-q", "-m", "tamper")
        self.assertTrue(any("artifact_sha256" in problem for problem in self._verify()))

    def test_verification_rejects_wrong_corekit_release_and_bad_observation(self) -> None:
        self._record()
        report = json.loads((self.repo / REPORT).read_text())
        report["observed"]["exit_code"] = 1
        del report["rust_build"]["target"]
        (self.repo / REPORT).write_text(json.dumps(report))
        self._commit_evidence()
        problems = self._verify(corekit_tag="v0.18.0")
        self.assertTrue(any("CoreKit release" in problem for problem in problems))
        self.assertTrue(any("observed needs" in problem for problem in problems))
        self.assertTrue(any("rust_build needs" in problem for problem in problems))

    def test_verification_requires_the_evidence_commit_not_the_source(self) -> None:
        self.assertIn("HEAD is the source commit", self._verify()[0])

    def test_verification_rejects_a_dirty_tree(self) -> None:
        self._record()
        self._commit_evidence()
        (self.repo / "stray.txt").write_text("x")
        self.assertTrue(any("changes" in problem for problem in self._verify()))

    def test_refuses_dirty_checkout_and_wrong_head(self) -> None:
        (self.repo / "stray.txt").write_text("x")
        with self.assertRaisesRegex(ValueError, "tracked or untracked changes"):
            self._record()
        (self.repo / "stray.txt").unlink()
        with self.assertRaisesRegex(ValueError, "not the source commit"):
            self._record(source_commit="1" * 40)
        self.assertEqual((self.repo / ARTIFACT).read_bytes(), b"old binary")

    def test_refuses_tag_that_does_not_match_workspace_version(self) -> None:
        with self.assertRaisesRegex(ValueError, "workspace version"):
            self._record(tag="v9.9.8")
        with self.assertRaisesRegex(ValueError, "vMAJOR.MINOR.PATCH"):
            self._record(tag="9.9.9")

    def test_refuses_rustflags_and_wrappers(self) -> None:
        with self.assertRaisesRegex(ValueError, "RUSTFLAGS"):
            record(self.repo, "v9.9.9", self.source, COREKIT_TAG, COREKIT_COMMIT, self.target,
                   runner=FakeToolchain(), base_env={"PATH": "/usr/bin", "RUSTFLAGS": "-C opt-level=1"})

    def test_failed_build_or_observation_writes_no_report(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaisesRegex(ValueError, "cargo build exited 101"):
                self._record(FakeToolchain(build_exit=101))
        self.assertEqual((self.repo / REPORT).read_text(), '{"old": true}\n')
        with self.assertRaisesRegex(ValueError, "`version` exited 3"):
            self._record(FakeToolchain(version_exit=3))
        self.assertEqual((self.repo / REPORT).read_text(), '{"old": true}\n')

    def test_corekit_release_resolves_from_tag_and_rejects_mismatch(self) -> None:
        corekit, commit = make_repo(self.root / "ck")
        git(corekit, "tag", "v0.17.0")
        runner = FakeToolchain()
        self.assertEqual(resolve_corekit_release(runner, "v0.17.0", None, corekit), ("v0.17.0", commit))
        self.assertEqual(resolve_corekit_release(runner, "v0.17.0", commit, corekit), ("v0.17.0", commit))
        with self.assertRaisesRegex(ValueError, "resolves to"):
            resolve_corekit_release(runner, "v0.17.0", COREKIT_COMMIT, corekit)
        with self.assertRaisesRegex(ValueError, "--corekit-commit or --corekit-root"):
            resolve_corekit_release(runner, "v0.17.0", None, None)
        with self.assertRaisesRegex(ValueError, "40-hex"):
            resolve_corekit_release(runner, "v0.17.0", "abc", None)

    def test_main_records_then_verifies(self) -> None:
        common = [
            "--repo-root", str(self.repo), "--tag", "v9.9.9", "--source-commit", self.source,
            "--corekit-tag", COREKIT_TAG, "--corekit-commit", COREKIT_COMMIT,
        ]
        toolchain = FakeToolchain()
        old_env = os.environ.pop("RUSTFLAGS", None)
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main([*common, "--target-dir", str(self.target)], runner=toolchain), 0)
                self._commit_evidence()
                self.assertEqual(main([*common, "--verify"], runner=toolchain), 0)
            (self.repo / ARTIFACT).write_bytes(b"x")
            git(self.repo, "add", ARTIFACT)
            git(self.repo, "commit", "-q", "-m", "tamper")
            with contextlib.redirect_stderr(io.StringIO()) as stderr:
                self.assertEqual(main([*common, "--verify"], runner=toolchain), 1)
            self.assertIn("artifact_sha256", stderr.getvalue())
        finally:
            if old_env is not None:
                os.environ["RUSTFLAGS"] = old_env

    def test_constants_name_the_shipped_binary(self) -> None:
        manifest = (ROOT / "crates" / "symeraseme-cli" / "Cargo.toml").read_text()
        self.assertIn(f'name = "{rse.PACKAGE}"', manifest)
        self.assertIn(f'name = "{rse.BIN_NAME}"', manifest)
        workflow = (ROOT / ".github" / "workflows" / "rust-prerelease.yml").read_text()
        self.assertIn(f"-p {rse.PACKAGE} --bin {rse.BIN_NAME}", workflow)


if __name__ == "__main__":
    unittest.main()
