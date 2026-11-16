#!/usr/bin/env python3
"""Record the Rust standalone release evidence CoreKit's consumer verifier reads.

Run this once per release, from a clean checkout of the version-prep commit
``P`` (the *source commit*).  It builds the shipped Rust CLI with
``cargo build --locked --release``, copies the binary to
``evidence/artifact/symeraseme``, runs ``version`` on it in an empty
environment and writes ``evidence/standalone.json``.  Only those two files
change, so the evidence commit ``E`` that follows ``P`` is the commit that is
tagged and recorded by CoreKit as ``consumer_release``.

``--verify`` re-checks committed evidence at ``E`` (no build) using the same
rules as ``port/consumer/verify.py`` in symaira-corekit, before the tag exists.

The ``tag``/``commit`` fields of the report name the CoreKit release the Rust
pin is bound to (``rust.release`` in CoreKit's ``docs/consumers.json``), not
this repository's release; the verifier compares them to that record.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping, Sequence

ROOT = Path(__file__).resolve().parents[1]

PACKAGE = "symeraseme-cli"
BIN_NAME = "symeraseme-rust"
REPORT = "evidence/standalone.json"
ARTIFACT = "evidence/artifact/symeraseme"
BUILD_COMMAND = ["cargo", "build", "--locked", "--release", "-p", PACKAGE, "--bin", BIN_NAME]
# Compiler inputs that would make the recorded build differ from the CI build.
FORBIDDEN_BUILD_ENV = (
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_TARGET",
)

REVISION_RE = re.compile(r"^[0-9a-f]{40}$")
TAG_RE = re.compile(r"^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


@dataclass(frozen=True)
class CommandResult:
    argv: tuple[str, ...]
    exit_code: int
    stdout: str
    stderr: str


Runner = Callable[[Sequence[str], Path, "Mapping[str, str] | None"], CommandResult]


def run_command(argv: Sequence[str], cwd: Path, env: Mapping[str, str] | None = None) -> CommandResult:
    completed = subprocess.run(
        list(argv), cwd=cwd, env=None if env is None else dict(env), check=False,
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        encoding="utf-8", errors="replace",
    )
    return CommandResult(tuple(argv), completed.returncode, completed.stdout, completed.stderr)


def _git(runner: Runner, repo: Path, *args: str) -> CommandResult:
    return runner(["git", *args], repo, None)


def _git_ok(runner: Runner, repo: Path, *args: str) -> str:
    result = _git(runner, repo, *args)
    if result.exit_code != 0:
        raise ValueError(f"git {' '.join(args)} failed: {result.stderr.strip() or result.exit_code}")
    return result.stdout.strip()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _regular_file(path: Path) -> bool:
    try:
        return stat.S_ISREG(path.lstat().st_mode)
    except OSError:
        return False


def workspace_version(repo: Path) -> str:
    document = tomllib.loads((repo / "Cargo.toml").read_text(encoding="utf-8"))
    version = document.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str):
        raise ValueError("Cargo.toml has no [workspace.package] version")
    return version


def _validate_inputs(repo: Path, tag: str, source_commit: str) -> None:
    if not TAG_RE.fullmatch(tag):
        raise ValueError(f"release tag must look like vMAJOR.MINOR.PATCH: {tag!r}")
    if not REVISION_RE.fullmatch(source_commit):
        raise ValueError("source commit must be a full 40-hex lowercase commit id")
    expected = f"v{workspace_version(repo)}"
    if tag != expected:
        raise ValueError(f"release tag {tag} does not match the workspace version ({expected})")


def resolve_corekit_release(
    runner: Runner, tag: str, commit: str | None, corekit_root: Path | None,
) -> tuple[str, str]:
    """Return the CoreKit release tag/commit pair the Rust pin is recorded under."""
    if not TAG_RE.fullmatch(tag):
        raise ValueError(f"CoreKit release tag must look like vMAJOR.MINOR.PATCH: {tag!r}")
    if commit is None:
        if corekit_root is None:
            raise ValueError("pass --corekit-commit or --corekit-root to name the CoreKit release commit")
        commit = _git_ok(runner, corekit_root, "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
    elif corekit_root is not None:
        resolved = _git_ok(runner, corekit_root, "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
        if resolved != commit:
            raise ValueError(f"CoreKit tag {tag} resolves to {resolved}, not {commit}")
    if not REVISION_RE.fullmatch(commit):
        raise ValueError("CoreKit release commit must be a full 40-hex lowercase commit id")
    return tag, commit


def require_clean_checkout(runner: Runner, repo: Path, source_commit: str) -> None:
    head = _git_ok(runner, repo, "rev-parse", "--verify", "HEAD^{commit}")
    if head != source_commit:
        raise ValueError(f"checkout HEAD {head} is not the source commit {source_commit}")
    status = _git_ok(runner, repo, "status", "--porcelain=v1", "--untracked-files=all")
    if status:
        raise ValueError("checkout has tracked or untracked changes; evidence needs a clean source commit")


def _build_env(base: Mapping[str, str], target_dir: Path) -> dict[str, str]:
    present = [name for name in FORBIDDEN_BUILD_ENV if base.get(name)]
    if present:
        raise ValueError(f"unset {', '.join(present)}: the recorded build must be a plain locked release build")
    env = dict(base)
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["CARGO_TERM_COLOR"] = "never"
    env["CARGO_TERM_PROGRESS_WHEN"] = "never"
    return env


def _version_line(result: CommandResult, prefix: str) -> str:
    line = result.stdout.strip().splitlines()[0] if result.stdout.strip() else ""
    if result.exit_code != 0 or not line.startswith(prefix):
        raise ValueError(f"cannot read toolchain version from {' '.join(result.argv)}")
    return line


def _rustc_host(result: CommandResult) -> str:
    for line in result.stdout.splitlines():
        if line.startswith("host: ") and line[6:].strip():
            return line[6:].strip()
    raise ValueError("rustc -vV did not report a host target")


def record(
    repo: Path, tag: str, source_commit: str, corekit_tag: str, corekit_commit: str,
    target_dir: Path, *, runner: Runner = run_command, base_env: Mapping[str, str] | None = None,
) -> dict[str, object]:
    """Build, observe and write the evidence files; return the report object."""
    _validate_inputs(repo, tag, source_commit)
    require_clean_checkout(runner, repo, source_commit)
    env = _build_env(os.environ if base_env is None else base_env, target_dir)

    cargo_version = _version_line(runner(["cargo", "--version"], repo, env), "cargo ")
    rustc_version = _version_line(runner(["rustc", "--version"], repo, env), "rustc ")
    host = _rustc_host(runner(["rustc", "-vV"], repo, env))
    lock_sha = sha256_file(repo / "Cargo.lock")

    build = runner(BUILD_COMMAND, repo, env)
    if build.exit_code != 0:
        sys.stderr.write(build.stderr)
        raise ValueError(f"cargo build exited {build.exit_code}; no evidence written")
    built = target_dir / "release" / BIN_NAME
    if not _regular_file(built):
        raise ValueError(f"cargo build did not produce {built}")

    artifact = repo / ARTIFACT
    artifact.parent.mkdir(parents=True, exist_ok=True)
    if artifact.is_symlink():
        raise ValueError(f"{ARTIFACT} must not be a symlink")
    staged = artifact.with_name(artifact.name + ".new")
    shutil.copyfile(built, staged)
    staged.chmod(0o755)
    os.replace(staged, artifact)

    with tempfile.TemporaryDirectory(prefix="ev-home-") as home:
        argv = ["env", "-i", "PATH=/usr/bin:/bin", f"HOME={home}", f"./{ARTIFACT}", "version"]
        observed = runner(argv, repo, None)
    if observed.exit_code != 0:
        raise ValueError(f"standalone `version` exited {observed.exit_code}: {observed.stderr.strip()}")

    report: dict[str, object] = {
        "tag": corekit_tag,
        "commit": corekit_commit,
        "artifact": ARTIFACT,
        "artifact_sha256": sha256_file(artifact),
        "observed": {
            "command": shlex.join(argv),
            "exit_code": observed.exit_code,
            "stdout": observed.stdout,
            "stderr": observed.stderr,
        },
        "rust_build": {
            "source_commit": source_commit,
            "cargo_version": cargo_version,
            "rustc_version": rustc_version,
            "target": host,
            "cargo_lock_sha256": lock_sha,
            "observed": {
                "command": list(BUILD_COMMAND),
                "exit_code": build.exit_code,
                "stdout": build.stdout,
                "stderr": build.stderr,
            },
        },
    }
    (repo / REPORT).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def verify_committed(
    repo: Path, tag: str, source_commit: str, corekit_tag: str, corekit_commit: str,
    *, runner: Runner = run_command,
) -> list[str]:
    """Return problems found in the evidence committed at HEAD (empty means accepted).

    Mirrors port/consumer/verify.py: report/artifact tracked and committed,
    report tag/commit equal to the CoreKit release, artifact digest, and Rust
    build provenance whose source commit precedes HEAD with only the evidence
    files changed in between.
    """
    problems: list[str] = []
    try:
        _validate_inputs(repo, tag, source_commit)
        require_clean_checkout(runner, repo, _git_ok(runner, repo, "rev-parse", "HEAD^{commit}"))
    except ValueError as error:
        return [str(error)]
    head = _git_ok(runner, repo, "rev-parse", "HEAD^{commit}")
    if head == source_commit:
        return ["HEAD is the source commit; verify the evidence commit that follows it"]

    for relative in (REPORT, ARTIFACT):
        if not _regular_file(repo / relative):
            problems.append(f"{relative} is not a regular non-symlink file")
        elif _git(runner, repo, "ls-files", "--error-unmatch", "--", relative).exit_code != 0:
            problems.append(f"{relative} is not tracked")
    if problems:
        return problems
    try:
        report = json.loads((repo / REPORT).read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        return [f"{REPORT} is not valid JSON: {error}"]
    if not isinstance(report, dict):
        return [f"{REPORT} must contain an object"]

    if report.get("tag") != corekit_tag or report.get("commit") != corekit_commit:
        problems.append("report tag/commit do not equal the CoreKit release")
    if report.get("artifact") != ARTIFACT:
        problems.append("report artifact is not " + ARTIFACT)
    digest = report.get("artifact_sha256")
    if not isinstance(digest, str) or not SHA256_RE.fullmatch(digest) or digest != sha256_file(repo / ARTIFACT):
        problems.append("artifact_sha256 does not match the committed artifact")
    observed = report.get("observed")
    if (
        not isinstance(observed, dict)
        or set(observed) != {"command", "exit_code", "stdout", "stderr"}
        or not isinstance(observed.get("command"), str) or not observed["command"].strip()
        or observed.get("exit_code") != 0
        or not isinstance(observed.get("stdout"), str) or not isinstance(observed.get("stderr"), str)
    ):
        problems.append("observed needs command, exit_code 0, stdout and stderr")
    elif not observed["command"].startswith("env -i ") or not observed["command"].endswith(f"./{ARTIFACT} version"):
        problems.append("observed command is not the empty-environment `version` run of the artifact")

    build = report.get("rust_build")
    if not isinstance(build, dict):
        return problems + ["rust_build provenance is missing"]
    if build.get("source_commit") != source_commit:
        problems.append("rust_build.source_commit is not the source commit")
    if _git(runner, repo, "merge-base", "--is-ancestor", source_commit, head).exit_code != 0:
        problems.append("source commit is not an ancestor of HEAD")
    changed = set(_git_ok(runner, repo, "diff", "--name-only", source_commit, head, "--").splitlines())
    extra = sorted(changed - {REPORT, ARTIFACT})
    if extra:
        problems.append("files other than the evidence changed after the source commit: " + ", ".join(extra))
    command = None
    build_observed = build.get("observed")
    if isinstance(build_observed, dict):
        command = build_observed.get("command")
    if (
        not isinstance(build.get("cargo_version"), str) or not build["cargo_version"].startswith("cargo ")
        or not isinstance(build.get("rustc_version"), str) or not build["rustc_version"].startswith("rustc ")
        or not isinstance(build.get("target"), str) or not build["target"].strip()
        or not isinstance(command, list) or not all(isinstance(arg, str) for arg in command)
        or command[:2] != ["cargo", "build"] or "--locked" not in command or "--release" not in command
        or not isinstance(build_observed, dict) or build_observed.get("exit_code") != 0
        or not isinstance(build_observed.get("stdout"), str) or not isinstance(build_observed.get("stderr"), str)
    ):
        problems.append("rust_build needs an observed locked release build with cargo/rustc versions and target")
    lock = build.get("cargo_lock_sha256")
    lock_changed = _git(runner, repo, "diff", "--quiet", source_commit, head, "--", "Cargo.lock").exit_code != 0
    if lock_changed or not isinstance(lock, str) or lock != sha256_file(repo / "Cargo.lock"):
        problems.append("cargo_lock_sha256 does not match Cargo.lock at HEAD and the source commit")
    return problems


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tag", required=True, help="release tag of this repository, e.g. v0.18.0 (must equal the workspace version)")
    parser.add_argument("--source-commit", required=True, help="40-hex version-prep commit P the binary is built from")
    parser.add_argument("--corekit-tag", required=True, help="CoreKit release tag in the consumer record's rust.release")
    parser.add_argument("--corekit-commit", help="CoreKit release commit in rust.release (resolved from --corekit-root when omitted)")
    parser.add_argument("--corekit-root", type=Path, help="CoreKit checkout used to resolve/confirm the CoreKit release commit")
    parser.add_argument("--target-dir", type=Path, help="cargo target directory (default: build/rust-evidence; reuse only if you accept a warm build log)")
    parser.add_argument("--repo-root", type=Path, default=ROOT)
    parser.add_argument("--verify", action="store_true", help="check committed evidence at HEAD (the evidence commit); builds nothing")
    return parser


def main(argv: list[str] | None = None, *, runner: Runner = run_command) -> int:
    args = _parser().parse_args(argv)
    repo = args.repo_root.resolve()
    try:
        corekit_tag, corekit_commit = resolve_corekit_release(
            runner, args.corekit_tag, args.corekit_commit,
            args.corekit_root.resolve() if args.corekit_root else None,
        )
        if args.verify:
            problems = verify_committed(repo, args.tag, args.source_commit, corekit_tag, corekit_commit, runner=runner)
            for problem in problems:
                print(f"record-standalone-evidence: {problem}", file=sys.stderr)
            if problems:
                return 1
            print(f"Evidence at HEAD satisfies the CoreKit standalone rules for {args.tag}; tag this commit.")
            return 0
        target_dir = (args.target_dir or repo / "build" / "rust-evidence").resolve()
        report = record(repo, args.tag, args.source_commit, corekit_tag, corekit_commit, target_dir, runner=runner)
    except (OSError, ValueError) as error:
        print(f"record-standalone-evidence: {error}", file=sys.stderr)
        return 1
    print(f"Wrote {REPORT} and {ARTIFACT} (sha256 {report['artifact_sha256']}).")
    print(f"Next: git add {REPORT} {ARTIFACT} && git commit, then rerun with --verify and tag that commit {args.tag}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
