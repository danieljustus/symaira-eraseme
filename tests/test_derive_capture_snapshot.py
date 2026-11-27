#!/usr/bin/env python3
"""Snapshot derivation controls and a replay against the real predecessor pair."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    'derive_capture_snapshot', ROOT / 'scripts/derive_capture_snapshot.py')
if SPEC is None or SPEC.loader is None:
    raise RuntimeError('Cannot load capture snapshot derivation')
DERIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DERIVE)

CANDIDATE = '5f5b27a2802395596dbb59b0362bc7fd71ed9b4a'
PREDECESSOR = 'a8eda4f77ce938c32d18dbad7d45de148b7d0482'
SNAPSHOT = '17c7d69658eeb22c4ac96a3c0d4facfb6a372274'
TAG = 'frozen-go-captures/dependency-refresh-a8eda4f7'


def git(*args, cwd=ROOT):
    return subprocess.check_output(['git', '-C', str(cwd), *args])


class CaptureSnapshotDerivation(unittest.TestCase):
    def test_constants_and_single_literal_override(self):
        self.assertEqual(DERIVE.RETIREMENT,
                         '22615ae67325c581fbe60cb0f2d3d2b2139cf900')
        self.assertEqual(DERIVE.PRODUCT_LITERAL, b'version: env!("CARGO_PKG_VERSION"),')
        self.assertEqual(DERIVE.CAPTURE_LITERAL, b'version: "dev",')
        original = b'name: "symeraseme",\n                version: env!("CARGO_PKG_VERSION"),\n'
        expected = b'name: "symeraseme",\n                version: "dev",\n'
        self.assertEqual(DERIVE.replace_once(original, DERIVE.PRODUCT_LITERAL,
                                            DERIVE.CAPTURE_LITERAL, 'protocol'), expected)
        with self.assertRaisesRegex(DERIVE.DerivationError, 'exactly one'):
            DERIVE.replace_once(original + original, DERIVE.PRODUCT_LITERAL,
                                DERIVE.CAPTURE_LITERAL, 'protocol')

    def test_predecessor_pair_produces_the_committed_snapshot_tree(self):
        # refresh-a8eda4f7 is the first #1330/#1334-generation snapshot and the
        # immediate predecessor of the genuine refresh-5f5b27a2 pair.
        predecessor, candidate, committed = TAG, CANDIDATE, SNAPSHOT
        with self.subTest(predecessor=predecessor):
                with tempfile.TemporaryDirectory(prefix='capture-snapshot-test-') as directory:
                    scratch = Path(directory) / 'worktree'
                    head_before = git('rev-parse', 'HEAD')
                    git('worktree', 'add', '-q', '--detach', str(scratch), candidate)
                    try:
                        derived = DERIVE.derive(ROOT, scratch, candidate, predecessor)
                        derived_tree = git('rev-parse', derived + '^{tree}').decode().strip()
                        committed_tree = git('rev-parse', committed + '^{tree}').decode().strip()
                        self.assertEqual(derived_tree, committed_tree)
                        command_list, tag = DERIVE.commands(ROOT, candidate, derived)
                        self.assertEqual(tag, 'frozen-go-captures/dependency-refresh-'
                                         + candidate[:8])
                        self.assertEqual(len(command_list), 3)
                        self.assertIn('tag -a ' + tag, command_list[0])
                        self.assertIn('push origin refs/tags/' + tag, command_list[1])
                        self.assertIn('gh workflow run frozen-http-aux-capture.yml',
                                      command_list[2])
                        self.assertIn(derived, command_list[0])
                        self.assertEqual(git('rev-parse', 'HEAD'), head_before)
                    finally:
                        git('worktree', 'remove', '--force', str(scratch))

    def test_comparator_override_table_ports_only_chrono_calls(self):
        # #1335: the edits apply exactly `count` times to the 22615ae bytes and
        # leave no chrono reference; the replay candidate above still locks
        # chrono, so it carries no override.
        candidate = git('rev-parse', 'HEAD').decode().strip()
        table = DERIVE.comparator_overrides(ROOT, candidate)
        self.assertEqual(sorted(table), [
            'crates/symeraseme-cli/tests/mcp_http_headers_process.rs',
            'crates/symeraseme-cli/tests/plan_execute_live_process.rs',
            'crates/symeraseme-core/tests/campaign_smtp.rs',
        ])
        for path, edits in table.items():
            ported = DERIVE.comparator_blob(ROOT, path, edits)
            self.assertNotIn(b'chrono', ported, path)
            self.assertNotEqual(ported, DERIVE.blob_at(ROOT, DERIVE.RETIREMENT, path))
        self.assertEqual(DERIVE.comparator_overrides(ROOT, CANDIDATE), {})
        with self.assertRaisesRegex(DERIVE.DerivationError, 'expected 2 of'):
            DERIVE.apply_overrides(b'now.to_utc(),', [
                {'old': 'now.to_utc(),', 'new': 'now,', 'count': 2}], 'campaign')

    def test_dirty_or_primary_checkout_is_refused(self):
        with self.assertRaisesRegex(DERIVE.DerivationError, 'must not be the caller checkout'):
            DERIVE.derive(ROOT, ROOT, CANDIDATE, TAG)
        with tempfile.TemporaryDirectory(prefix='capture-snapshot-test-') as directory:
            scratch = Path(directory) / 'worktree'
            git('worktree', 'add', '-q', '--detach', str(scratch), CANDIDATE)
            try:
                (scratch / 'dirty-probe').write_text('untracked\n', encoding='utf-8')
                with self.assertRaisesRegex(DERIVE.DerivationError, 'scratch worktree must be clean'):
                    DERIVE.derive(ROOT, scratch, CANDIDATE, TAG)
            finally:
                git('worktree', 'remove', '--force', str(scratch))


if __name__ == '__main__':
    unittest.main()
