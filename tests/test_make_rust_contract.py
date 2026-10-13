#!/usr/bin/env python3
"""Synthetic Make/coverage failure controls, not measured Rust coverage."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class RustMakeContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='rust-make-contract-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copyfile(ROOT / 'Makefile', self.root / 'Makefile')
        (self.root / 'scripts').mkdir()
        shutil.copyfile(ROOT / 'scripts/check-rust-coverage.sh', self.root / 'scripts/check-rust-coverage.sh')
        source = self.root / 'crates/example/src/mcp/check.rs'
        source.parent.mkdir(parents=True)
        source.write_text('fn check() {}\n')
        self.source = source
        cargo = self.root / 'fake cargo'
        cargo.write_text('#!' + sys.executable + '\n' + '''import json, os, pathlib, sys
with open(os.environ['CALLS'], 'a') as out:
    out.write(json.dumps(sys.argv[1:]) + '\\n')
if os.environ.get('FAIL'):
    raise SystemExit(23)
if 'llvm-cov' in sys.argv:
    destination = pathlib.Path(sys.argv[sys.argv.index('--output-path') + 1])
    destination.write_text(os.environ['REPORT'])
''')
        cargo.chmod(0o700)
        self.calls = self.root / 'calls.jsonl'
        self.env = dict(os.environ, CARGO=str(cargo), CALLS=str(self.calls), CARGO_TARGET_DIR=str(self.root / 'target space'))
        self.env.pop('FAIL', None)
        self.report = {'data': [{'files': [{'filename': str(source), 'summary': {'lines': {'count': 10, 'covered': 9}}}]}]}

    def run_make(self, target):
        self.env['REPORT'] = json.dumps(self.report)
        return subprocess.run(['make', '-f', 'Makefile', target], cwd=self.root,
                              env=self.env, text=True, capture_output=True, timeout=20)

    def test_workspace_tests_and_doctests_are_real_commands_even_with_same_named_file(self):
        (self.root / 'test').touch()
        result = self.run_make('test')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()]
        self.assertEqual(calls, [
            ['test', '--workspace', '--all-targets', '--all-features', '--locked'],
            ['test', '--workspace', '--doc', '--all-features', '--locked'],
        ])

    def test_cargo_failure_stops_before_doctests(self):
        self.env['FAIL'] = '1'
        self.assertNotEqual(self.run_make('test').returncode, 0)
        self.assertEqual(len(self.calls.read_text().splitlines()), 1)

    def test_clean_never_uses_configurable_output_paths(self):
        overrides = {
            'CARGO_TARGET_DIR': str(self.root / 'external cache'),
            'RUST_TARGET_DIR': str(self.root / 'other project'),
            'BINARY': str(self.root / 'external binary'),
            'COVERAGE_FILE': str(self.root / 'external coverage.json'),
        }
        for arguments in ([], [f'{key}={value}' for key, value in overrides.items()]):
            with self.subTest(arguments=arguments):
                result = subprocess.run(
                    ['make', '-n', 'clean', *arguments], cwd=self.root,
                    env=dict(self.env, **overrides), text=True,
                    capture_output=True, timeout=20,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines(), [
                    'rm -f -- symeraseme',
                    'rm -rf -- build/rust dist',
                ])

    def test_retired_race_target_does_not_fake_success(self):
        result = self.run_make('test-race')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Go -race is retired', result.stderr)
        self.assertFalse(self.calls.exists())

    def test_coverage_preserves_global_and_critical_thresholds(self):
        result = self.run_make('coverage')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('90.00%', result.stdout)
        call = json.loads(self.calls.read_text())
        self.assertEqual(call[call.index('--fail-under-lines') + 1], '80')
        self.report['data'][0]['files'][0]['summary']['lines']['covered'] = 8
        self.assertNotEqual(self.run_make('coverage').returncode, 0)

    def test_coverage_rejects_missing_duplicate_zero_and_wrong_type(self):
        good = json.loads(json.dumps(self.report['data'][0]['files'][0]))
        cases = [[], [good, good]]
        for counts in [{'count': 0, 'covered': 0}, {'count': True, 'covered': 1}, {'count': 10, 'covered': 11}]:
            bad = json.loads(json.dumps(good))
            bad['summary']['lines'] = counts
            cases.append([bad])
        for files in cases:
            with self.subTest(files=files):
                self.report['data'][0]['files'] = files
                self.assertNotEqual(self.run_make('coverage').returncode, 0)

    def test_ci_uses_shared_coverage_gate_and_rejects_no_critical_code(self):
        workflow = (ROOT / '.github/workflows/rust-ci.yml').read_text()
        self.assertIn('run: bash scripts/check-rust-coverage.sh', workflow)
        self.assertNotIn('critical_pattern=', workflow)
        self.source.write_text('// Declaration-only facade.\npub mod child;\n')
        result = self.run_make('coverage')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('No critical Rust files found', result.stderr)
        self.assertFalse(self.calls.exists())

    def test_coverage_propagates_instrumentation_failure(self):
        self.env['FAIL'] = '1'
        self.assertNotEqual(self.run_make('coverage').returncode, 0)


if __name__ == '__main__':
    unittest.main()
