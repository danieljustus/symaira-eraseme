#!/usr/bin/env python3
"""Freeze/check Go 1.26.6's Windows filepath Unicode comparison data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
TOOLCHAIN = "go1.26.6"


def generate():
    env = dict(os.environ, GOTOOLCHAIN=TOOLCHAIN)
    goroot = Path(subprocess.check_output(["go", "env", "GOROOT"], env=env, text=True).strip())
    raw = subprocess.check_output(["go", "run", str(HERE / "fold.go")], cwd=ROOT, env=env)
    oracle = json.loads(raw)
    assert oracle["toolchain"] == TOOLCHAIN
    sources = ["path/filepath/path_windows.go", "path/filepath/path.go",
               "strings/strings.go", "unicode/letter.go", "unicode/tables.go"]
    oracle["sources"] = {
        name: hashlib.sha256((goroot / "src" / name).read_bytes()).hexdigest()
        for name in sources
    }
    oracle["generator_sha256"] = {
        name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
        for name in ("fold.go", "generate_fold.py")
    }
    pairs = oracle["pairs"]
    assert pairs and len({p[0] for p in pairs}) == len(pairs)
    ranges = []
    i = 0
    while i < len(pairs):
        start, mapped = pairs[i]
        delta = start - mapped
        step = pairs[i + 1][0] - start if i + 1 < len(pairs) else 1
        if step not in (1, 2):
            step = 1
        end = start
        j = i + 1
        while j < len(pairs) and pairs[j][0] == end + step and pairs[j][0] - pairs[j][1] == delta:
            end = pairs[j][0]
            j += 1
        ranges.append((start, end, step, delta))
        i = j
    # Range compression may not introduce mappings for any absent codepoint.
    expanded = {r: r - delta for start, end, step, delta in ranges
                for r in range(start, end + 1, step)}
    assert expanded == dict(pairs)
    lines = [
        "// Generated from Go 1.26.6 unicode.SimpleFold, Unicode " + oracle["unicode"] + ".",
        "// Regenerate/check: python3 rust-tests/parity/oracle/migration-safety/generate_fold.py [--check]",
        "// Windows filepath.sameWord uses strings.EqualFold; ASCII-only/lowercase comparisons differ.",
        "#[rustfmt::skip]",
        "const RANGES: &[(u32, u32, u32, u32)] = &[",
    ]
    lines += [f"    (0x{a:X}, 0x{b:X}, {s}, 0x{d:X})," for a, b, s, d in ranges]
    lines += ["];"]
    lines += [
        "", "pub(super) fn fold(character: char) -> u32 {",
        "    let value = u32::from(character);",
        "    for &(start, end, step, delta) in RANGES {",
        "        if value < start {", "            break;", "        }",
        "        if value <= end && (value - start) % step == 0 {",
        "            return value - delta;", "        }", "    }", "    value", "}",
    ]
    return {
        HERE / "fold.json": (json.dumps(oracle, indent=2, ensure_ascii=True) + "\n").encode(),
        ROOT / "crates/symeraseme-engine/src/migration/windows_case_fold.rs": ("\n".join(lines) + "\n").encode(),
    }, len(pairs), len(ranges)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    outputs, pairs, ranges = generate()
    for path, data in outputs.items():
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                raise SystemExit(f"Go fold oracle drift: {path.relative_to(ROOT)}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    print(f"{'checked' if args.check else 'generated'} {pairs} Go fold mappings in {ranges} exact ranges")


if __name__ == "__main__":
    main()
