#!/usr/bin/env python3
"""Capture ID-005 through pinned Go production code; keep raw evidence external."""
import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

BASE = "93721a93ec9e527410c4ec3051779cf516216b72"
CONTRACT = "bf53346eec234929bedf0314b99e3da85dbb991b"
CASES = ["fresh", "nested", "existing_directory", "replacement", "rename_failure",
         "temp_failure", "mkdir_failure", "verify", "list", "wrong_command", "expired",
         "umask_000", "umask_077", "umask_777", "write_failure"]
FAULT_CASES = ["close_failure", "chmod_failure"]
# Insert calls before the original operations; never replace their result checks.
FAULT_HOOKS = {
    '\tif err := tmp.Close(); err != nil {': '\tid005BeforeClose(tmp)\n',
    '\tif err := os.Chmod(tmpName, mode); err != nil {': '\tid005BeforeChmod(tmpName)\n',
}


def instrument(original):
    modified = original
    for anchor, hook in FAULT_HOOKS.items():
        if modified.count(anchor) != 1:
            raise ValueError(f"expected exactly one pinned Go operation: {anchor!r}")
        modified = modified.replace(anchor, hook + anchor)
    return modified


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--fixture", type=Path, help="explicit privacy-reviewed derived output")
    parser.add_argument("--fault-probes", action="store_true",
                        help="instrument only the archived Go source for real close/chmod errors")
    args = parser.parse_args()
    cases = FAULT_CASES if args.fault_probes else CASES
    repo = Path(__file__).resolve().parents[2]
    evidence = args.evidence.resolve()
    if evidence.is_relative_to(repo):
        parser.error("raw observations must stay outside the worktree")
    evidence.mkdir(parents=True, exist_ok=False)

    def run(argv, name, **kwargs):
        result = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)
        (evidence / (name + ".stdout")).write_bytes(result.stdout)
        (evidence / (name + ".stderr")).write_bytes(result.stderr)
        (evidence / (name + ".exit")).write_text(str(result.returncode) + "\n")
        result.check_returncode()
        return result.stdout

    def git(*argv):
        return subprocess.check_output(["git", *argv], cwd=repo)

    # Tie the archived executable to both the assigned base and corrected oracle.
    sources = ["internal/identity/consent.go", "internal/identity/gate.go", "internal/identity/profile.go"]
    for source in sources[:2]:
        assert git("show", BASE + ":" + source) == git("show", CONTRACT + ":" + source)
    # Non-Go extensions prevent `go test ./...` from discovering the templates
    # as a standalone package that cannot access production identity internals.
    helpers = sorted(Path(__file__).parent.glob("*_test.go.in"))
    digest = lambda data: hashlib.sha256(data).hexdigest()
    provenance = {"base": BASE, "contract": CONTRACT,
                  "source_sha256": {p: digest(git("show", BASE + ":" + p)) for p in sources},
                  "helper_sha256": {p.name: digest(p.read_bytes()) for p in helpers},
                  "generator_sha256": digest(Path(__file__).read_bytes()), "cases": cases}
    env = {k: os.environ[k] for k in ("PATH", "HOME")}
    env.update(GOTOOLCHAIN="go1.26.6", GOPROXY="off", GOENV="off", GOWORK="off", CGO_ENABLED="0")
    # The cached compiler/modules are resolved before isolating application HOME.
    goenv = json.loads(run(["go", "env", "-json", "GOROOT", "GOMODCACHE", "GOCACHE", "GOOS", "GOARCH"], "go-env", env=env))
    go = str(Path(goenv["GOROOT"]) / "bin/go")
    provenance["toolchain"] = run([go, "version"], "go-version", env=env).decode().strip()
    provenance["platform"] = {k: goenv[k] for k in ("GOOS", "GOARCH")}
    with tempfile.TemporaryDirectory(prefix="id005-go-source-") as scratch:
        source = Path(scratch)
        archive = git("archive", BASE)
        provenance["archive_sha256"] = digest(archive)
        run(["tar", "-x", "-C", str(source)], "extract", input=archive)
        if args.fault_probes:
            consent = source / sources[0]
            original = consent.read_text()
            modified = instrument(original)
            # Negative control: source drift must fail before compilation.
            try:
                instrument(original.replace(next(iter(FAULT_HOOKS)), ""))
            except ValueError:
                pass
            else:
                raise AssertionError("instrumentation accepted missing source anchor")
            consent.write_text(modified)
            (evidence / "instrumentation.diff").write_text("".join(difflib.unified_diff(
                original.splitlines(keepends=True), modified.splitlines(keepends=True),
                fromfile="pinned/consent.go", tofile="probe/consent.go")))
            provenance["instrumentation"] = {
                "kind": "state-changing hooks before unchanged checked operations",
                "source_sha256": digest(modified.encode()), "hooks": FAULT_HOOKS,
            }
        for helper in helpers:
            (source / "internal/identity" / helper.stem).write_bytes(helper.read_bytes())
        env.update(HOME=str(evidence / "home"), GOMODCACHE=goenv["GOMODCACHE"], GOCACHE=goenv["GOCACHE"], TZ="UTC", LC_ALL="C")
        (evidence / "home").mkdir()
        binary = evidence / "consent-oracle.test"
        run([go, "test", "-c", "-o", str(binary), "./internal/identity"], "build", cwd=source, env=env)
        provenance["binary_sha256"] = digest(binary.read_bytes())
        derived = []
        for name in cases:
            runtime = evidence / (name + "-root")
            runtime.mkdir(mode=0o700)
            output = evidence / (name + ".raw.json")
            case_env = dict(env, ID005_ROOT=str(runtime), ID005_CASE=name, ID005_OUTPUT=str(output))
            mask = int(name.removeprefix("umask_"), 8) if name.startswith("umask_") else 0o022
            stdout = run([str(binary), "-test.run=^TestConsentID005Capture$", "-test.v", "-test.count=1"], name, cwd=source, env=case_env, umask=mask)
            assert stdout.count(b"--- PASS: TestConsentID005Capture (") == 1, stdout
            observation = json.loads(output.read_bytes())
            assert observation["error_class"] != "unclassified", observation["raw_error"]
            if args.fault_probes:
                expected_class = "closed_file" if name == "close_failure" else "not_found"
                assert observation["failed"] and observation["error_class"] == expected_class, observation
            # Side-effect comparison retains failure vs success. Native error text
            # (including temp paths) is preserved verbatim only in raw observations.
            del observation["raw_error"]
            derived.append(observation)
    (evidence / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    document = {"schema": "consent-id005-v1", "base": BASE, "source_sha256": provenance["source_sha256"], "cases": derived}
    if args.fault_probes:
        document.update(
            schema="consent-id005-faults-v1",
            instrumentation=provenance["instrumentation"],
            helper_sha256=provenance["helper_sha256"],
            generator_sha256=provenance["generator_sha256"],
            blockers={
                "delayed_close_io": "Preclosing exercises Go os.ErrClosed, not native delayed EIO/ENOSPC; no controlled faulty filesystem is available.",
                "chmod_existing_temp": "Unlinking exercises real Chmod ENOENT but cannot prove cleanup after chmod failure with the temporary file still present; no deterministic permission-fault hook is available.",
                "sync": "Go has no sync operation. Rust retains sync_all and returns its error before publishing; no Go sync-error fixture exists.",
                "non_unix_close": "Rust non-Unix File drop still discards close errors; a reviewed safe checked-close adapter and native proof remain required.",
            })
    data = (json.dumps(document, indent=2) + "\n").encode()
    (evidence / "derived.json").write_bytes(data)
    if args.fixture:
        args.fixture.write_bytes(data)
    print(f"Captured {len(derived)} Go cases; evidence: {evidence}")


if __name__ == "__main__":
    main()
