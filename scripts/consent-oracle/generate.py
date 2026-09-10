#!/usr/bin/env python3
"""Capture ID-005 through pinned Go production code; keep raw evidence external."""
import argparse
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
         "umask_000", "umask_077", "umask_777"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--fixture", type=Path, help="explicit privacy-reviewed derived output")
    args = parser.parse_args()
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
    sources = ["internal/identity/consent.go", "internal/identity/gate.go"]
    for source in sources:
        assert git("show", BASE + ":" + source) == git("show", CONTRACT + ":" + source)
    helper = Path(__file__).with_name("consent_id005_test.go")
    digest = lambda data: hashlib.sha256(data).hexdigest()
    provenance = {"base": BASE, "contract": CONTRACT,
                  "source_sha256": {p: digest(git("show", BASE + ":" + p)) for p in sources},
                  "helper_sha256": digest(helper.read_bytes()),
                  "generator_sha256": digest(Path(__file__).read_bytes()), "cases": CASES}
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
        (source / "internal/identity/consent_id005_test.go").write_bytes(helper.read_bytes())
        env.update(HOME=str(evidence / "home"), GOMODCACHE=goenv["GOMODCACHE"], GOCACHE=goenv["GOCACHE"], TZ="UTC", LC_ALL="C")
        (evidence / "home").mkdir()
        binary = evidence / "consent-oracle.test"
        run([go, "test", "-c", "-o", str(binary), "./internal/identity"], "build", cwd=source, env=env)
        provenance["binary_sha256"] = digest(binary.read_bytes())
        derived = []
        for name in CASES:
            runtime = evidence / (name + "-root")
            runtime.mkdir(mode=0o700)
            output = evidence / (name + ".raw.json")
            case_env = dict(env, ID005_ROOT=str(runtime), ID005_CASE=name, ID005_OUTPUT=str(output))
            mask = int(name.removeprefix("umask_"), 8) if name.startswith("umask_") else 0o022
            run([str(binary), "-test.run=^TestConsentID005Capture$", "-test.v", "-test.count=1"], name, cwd=source, env=case_env, umask=mask)
            observation = json.loads(output.read_bytes())
            # Side-effect comparison retains failure vs success. Native error text
            # (including temp paths) is preserved verbatim only in raw observations.
            del observation["raw_error"]
            derived.append(observation)
    (evidence / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    document = {"schema": "consent-id005-v1", "source_sha256": provenance["source_sha256"], "cases": derived}
    data = (json.dumps(document, indent=2) + "\n").encode()
    (evidence / "derived.json").write_bytes(data)
    if args.fixture:
        args.fixture.write_bytes(data)
    print(f"Captured {len(derived)} Go cases; evidence: {evidence}")


if __name__ == "__main__":
    main()
