#!/usr/bin/env python3
"""Capture/check scheduler read failures through pinned production migration.Run."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
PIN = "4e582f28"
SCENARIOS = ("generated-unreadable", "native-unreadable", "generated-directory", "native-directory")


def fold(value, root):
    if isinstance(value, str):
        return value.replace(str(root), "<ROOT>")
    if isinstance(value, list):
        return [fold(v, root) for v in value]
    if isinstance(value, dict):
        return {k: fold(v, root) for k, v in value.items()}
    return value


def generate():
    if os.name == "nt":
        raise SystemExit("These recorded POSIX read-failure cases require a Unix host")
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    paths = ["go.mod", "go.sum", "internal/migration", "internal/scheduler"]
    archive = subprocess.check_output(["git", "archive", PIN, *paths], cwd=ROOT, timeout=30)
    commit = subprocess.check_output(["git", "rev-parse", PIN], cwd=ROOT, text=True, timeout=10).strip()
    with tempfile.TemporaryDirectory(prefix="migration-io-go-", dir=target) as temporary:
        stage = Path(temporary)
        reference = stage / "reference"
        reference.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as tf:
            tf.extractall(reference, filter="data")
        sources = {str(p.relative_to(reference)): hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in sorted(reference.rglob("*.go")) if not p.name.endswith("_test.go")}
        sources.update({name: hashlib.sha256((reference / name).read_bytes()).hexdigest()
                        for name in ("go.mod", "go.sum")})
        adapter = reference / "migration_io_probe.go"
        adapter.write_bytes((HERE / "io.go").read_bytes())
        executable = stage / "oracle"
        env = dict(os.environ, GOTOOLCHAIN="go1.26.6", GOPROXY="off", GOSUMDB="off")
        subprocess.run(["go", "build", "-o", str(executable), str(adapter)],
                       cwd=reference, env=env, check=True, timeout=120)
        cases = []
        for scenario in SCENARIOS:
            root = stage / scenario
            root.mkdir()
            runtime = {"PATH": os.environ["PATH"], "LC_ALL": "C", "TZ": "UTC"}
            for variable in ("HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "TMP", "TEMP"):
                directory = root / variable
                directory.mkdir()
                runtime[variable] = str(directory)
            proc = subprocess.run([str(executable), scenario, str(root)], cwd=root,
                                  env=runtime, capture_output=True, timeout=10)
            if proc.returncode or proc.stderr:
                raise RuntimeError(f"{scenario}: Go adapter failed: {proc.stderr.decode(errors='replace')}")
            observation = fold(json.loads(proc.stdout), root)
            assert isinstance(observation, dict)
            assert observation["error"] and observation["report"] is None
            assert not observation["destination_exists"] and not observation["backup_exists"]
            cases.append({"id": scenario, "observation": observation})
        return {"reference": commit, "toolchain": "go1.26.6", "platform": "unix",
                "source_sha256": sources,
                "generator_sha256": {n: hashlib.sha256((HERE / n).read_bytes()).hexdigest()
                                     for n in ("io.go", "generate_io.py")}, "cases": cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = generate()
    data = (json.dumps(result, indent=2, sort_keys=True) + "\n").encode()
    destination = HERE / "io.json"
    if args.check:
        if not destination.is_file() or destination.read_bytes() != data:
            raise SystemExit("migration I/O oracle drift")
    else:
        destination.write_bytes(data)
    print(f"{'checked' if args.check else 'captured'} {len(result['cases'])} pinned Go migration I/O scenarios")


if __name__ == "__main__":
    main()
