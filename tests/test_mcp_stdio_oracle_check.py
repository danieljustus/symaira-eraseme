#!/usr/bin/env python3
"""Non-writing acceptance controls for the MCP stdio mutation oracle."""
import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "rust-tests/parity/oracle/mcp-stdio-mutations/generate.py"
spec = importlib.util.spec_from_file_location("mcp_stdio_oracle", GENERATOR)
assert spec is not None and spec.loader is not None
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)


class OracleCheckTests(unittest.TestCase):
    def test_check_accepts_only_matching_observations_without_writing(self):
        observed = json.loads(GENERATOR.with_name("cases.json").read_bytes())
        self.assertEqual(len(observed["cases"]), 670)
        self.assertEqual(len({c["name"] for c in observed["cases"]}), 670)
        self.assertEqual([c["name"] for c in oracle.case_specs()],
                         [c["name"] for c in observed["cases"]])
        variants = [("valid", copy.deepcopy(observed), False)]
        other_host = copy.deepcopy(observed)
        other_host["go_version"] = "go version go1.26.6 windows/arm64"
        variants.append(("native-host", other_host, False))
        for field, value in (("stdout_base64", "Y29ycnVwdA=="),
                             ("exit_code", False), ("stderr_base64", "Yg==")):
            altered = copy.deepcopy(observed)
            altered["cases"][0][field] = value
            variants.append((field, altered, True))
        for field in ("generator_sha256", "source_revision"):
            altered = copy.deepcopy(observed)
            altered[field] = "corrupt"
            variants.append((field, altered, True))
        missing = copy.deepcopy(observed)
        missing["cases"].pop()
        variants.append(("missing-case", missing, True))
        compiler = copy.deepcopy(observed)
        compiler["go_version"] = "go version go1.27.1 darwin/arm64"
        variants.append(("compiler", compiler, True))
        for version in ("go version go1.26.6", "go version go1.26.6 darwin/arm64 extra",
                        "go version go1.26.60 darwin/arm64", "go version go1.26.6 invalid",
                        False):
            altered = copy.deepcopy(observed)
            altered["go_version"] = version
            variants.append((f"malformed-version-{version}", altered, True))
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "cases.json"
            for name, expected, reject in variants:
                with self.subTest(name=name):
                    raw = (json.dumps(expected, indent=2) + "\n").encode()
                    target.write_bytes(raw)
                    if reject:
                        with self.assertRaises(ValueError):
                            oracle.check(target, observed)
                    else:
                        oracle.check(target, observed)
                    self.assertEqual(target.read_bytes(), raw)


if __name__ == "__main__":
    unittest.main()
