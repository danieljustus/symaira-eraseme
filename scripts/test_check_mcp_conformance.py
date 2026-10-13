#!/usr/bin/env python3
"""Unit controls for check-mcp-conformance.py acceptance logic.

All results here are SYNTHETIC unit controls to exercise verifier branches.
They are never conformance proof; real official execution runs in CI.
"""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent / 'check-mcp-conformance.py'
spec = importlib.util.spec_from_file_location('check_mcp_conformance', SCRIPT)
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)


def ok_result(scenario, checks=None):
    checks = checks if checks is not None else [{'status': 'SUCCESS', 'id': 'synthetic'}]
    return dict(binary='rust', scenario=scenario, exit_status=0, checks=checks)


def valid_results():
    return [ok_result(s) for s in harness.SCENARIOS]


class VerifyResultsTest(unittest.TestCase):
    def test_accepts_three_rust_scenarios_with_success_checks(self):
        harness.verify_results(valid_results())

    def test_rejects_go_binary_result(self):
        results = valid_results()
        results[0]['binary'] = 'go'
        with self.assertRaisesRegex(RuntimeError, 'Non-Rust'):
            harness.verify_results(results)

    def test_rejects_missing_scenario(self):
        with self.assertRaisesRegex(RuntimeError, 'inventory mismatch'):
            harness.verify_results(valid_results()[:2])

    def test_rejects_extra_or_reordered_scenarios(self):
        with self.assertRaisesRegex(RuntimeError, 'inventory mismatch'):
            harness.verify_results(valid_results() + [ok_result('ping')])
        with self.assertRaisesRegex(RuntimeError, 'inventory mismatch'):
            harness.verify_results(list(reversed(valid_results())))

    def test_rejects_nonzero_exit(self):
        results = valid_results()
        results[1]['exit_status'] = 1
        with self.assertRaisesRegex(RuntimeError, 'exited with 1'):
            harness.verify_results(results)

    def test_rejects_zero_official_checks(self):
        results = valid_results()
        results[2]['checks'] = []
        with self.assertRaisesRegex(RuntimeError, 'zero official checks'):
            harness.verify_results(results)

    def test_rejects_missing_checks_field(self):
        results = valid_results()
        del results[2]['checks']
        with self.assertRaisesRegex(RuntimeError, 'zero official checks'):
            harness.verify_results(results)

    def test_rejects_failed_or_malformed_check(self):
        results = valid_results()
        results[0]['checks'] = [{'status': 'SUCCESS'}, {'status': 'FAILURE'}]
        with self.assertRaisesRegex(RuntimeError, 'non-SUCCESS'):
            harness.verify_results(results)
        results[0]['checks'] = ['SUCCESS']
        with self.assertRaisesRegex(RuntimeError, 'non-SUCCESS'):
            harness.verify_results(results)


class ManifestAndBinaryTest(unittest.TestCase):
    def test_manifest_is_rust_only_without_go_fields(self):
        with tempfile.TemporaryDirectory() as temp:
            exe = Path(temp) / 'symeraseme'
            exe.write_bytes(b'synthetic-rust-binary')
            exe.chmod(0o700)
            m = harness.build_manifest('a' * 40, '1.27.1', 'v22.0.0', exe, valid_results(), '2026-10-13T00:00:00+00:00')
        self.assertEqual(list(m['binary_sha256']), ['rust'])
        self.assertNotIn('go_build_info', m)
        self.assertIn('Rust-only', m['scope'])
        self.assertNotIn('go', m['scope'].lower().replace('go executable', ''))
        self.assertEqual(m['implementation'], 'rust-only')

    def test_rejects_missing_or_non_executable_rust_binary(self):
        with tempfile.TemporaryDirectory() as temp:
            plain = Path(temp) / 'plain'
            plain.write_bytes(b'x')
            plain.chmod(0o600)
            with self.assertRaisesRegex(RuntimeError, 'not an executable'):
                harness.require_executable(plain)
            with self.assertRaisesRegex(RuntimeError, 'not an executable'):
                harness.require_executable(Path(temp) / 'absent')


class OptimizedModeTest(unittest.TestCase):
    def test_require_enforced_regardless_of_optimize(self):
        # Acceptance uses require(), not assert, so -O cannot strip it.
        with self.assertRaisesRegex(RuntimeError, 'boom'):
            harness.require(False, 'boom')
        results = valid_results()
        results[0]['checks'] = []
        with self.assertRaisesRegex(RuntimeError, 'zero official checks'):
            harness.verify_results(results)


if __name__ == '__main__':
    unittest.main()
