#!/usr/bin/env python3
"""Synthetic preparation controls and CI wiring, not native capture evidence."""
import importlib.util
from pathlib import Path
import re
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('frozen_history', ROOT / 'scripts/prepare-frozen-history.py')
if SPEC is None or SPEC.loader is None:
    raise RuntimeError('Cannot load frozen history preparation')
HISTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HISTORY)


class FrozenHistoryPreparation(unittest.TestCase):
    def test_preparation_fails_closed_and_precedes_every_affected_gate(self):
        # Derive the active frozen readers' Git inputs, not historical metadata
        # from unused captures. This guards the preparer's hand-written list.
        texts = (path.read_text() for path in ROOT.glob('crates/*/tests/support/frozen_*.rs'))
        readers = [text for text in texts if '"git"' in text]
        self.assertTrue(readers)
        required = set().union(*(set(re.findall(r'"([0-9a-f]{40})"', text)) for text in readers))
        stdio = re.findall(r'^SOURCE_REVISION = "([0-9a-f]{40})"$',
                           (ROOT / 'rust-tests/parity/oracle/mcp-stdio-mutations/generate.py').read_text(),
                           re.MULTILINE)
        self.assertEqual(len(stdio), 1)
        # Keep the historical producer available after readers select a refresh.
        historical = {'13ddc6a43036d6b34706db2b82bbb5a13c40d6de'}
        self.assertEqual(set(HISTORY.REVISIONS), required | set(stdio) | historical)
        self.assertEqual(len(HISTORY.REVISIONS), len(set(HISTORY.REVISIONS)))
        for revision in HISTORY.REVISIONS:
            self.assertRegex(revision, r'^[0-9a-f]{40}$')
        self.assertEqual(HISTORY.REPOSITORY, 'https://github.com/danieljustus/symaira-eraseme.git')

        # Simulated Git replies: prove read-only default, explicit fetch,
        # failed fetch propagation, post-fetch verification and offline reuse.
        for present, fetch, fetch_error, still_missing in (
            (False, False, False, False),
            (False, True, False, False),
            (False, True, True, False),
            (False, True, False, True),
            (True, False, False, False),
            (True, True, False, False),
        ):
            with self.subTest(present=present, fetch=fetch, fetch_error=fetch_error,
                              still_missing=still_missing):
                calls = []
                available = present

                def run(command, **kwargs):
                    nonlocal available
                    calls.append(command)
                    self.assertEqual(kwargs['cwd'], ROOT)
                    self.assertGreater(kwargs['timeout'], 0)
                    if 'fetch' in command:
                        self.assertTrue(kwargs['check'])
                        self.assertEqual(command, [
                            'git', '--no-replace-objects', '-c', 'fetch.fsckObjects=true',
                            'fetch', '--no-tags', '--no-write-fetch-head', '--no-recurse-submodules',
                            HISTORY.REPOSITORY, *HISTORY.REVISIONS,
                        ])
                        if fetch_error:
                            raise subprocess.CalledProcessError(1, command)
                        available = not still_missing
                        return subprocess.CompletedProcess(command, 0)
                    self.assertEqual(command[:4], ['git', '--no-replace-objects', 'cat-file', '-e'])
                    return subprocess.CompletedProcess(command, 0 if available else 1)

                with patch.object(HISTORY.subprocess, 'run', side_effect=run):
                    if fetch_error:
                        with self.assertRaises(subprocess.CalledProcessError):
                            HISTORY.prepare(ROOT, fetch)
                    elif not present and (not fetch or still_missing):
                        with self.assertRaisesRegex(RuntimeError, 'Missing frozen producer objects'):
                            HISTORY.prepare(ROOT, fetch)
                    else:
                        HISTORY.prepare(ROOT, fetch)
                self.assertEqual(sum('fetch' in call for call in calls), int(fetch and not present))
                self.assertEqual(sum('cat-file' in call for call in calls),
                                 len(HISTORY.REVISIONS) * (2 if fetch and not present and not fetch_error else 1))

        # Fixed owning-job inventory. This is structural protection; the real
        # clean-clone/native gates must still execute all original comparisons.
        jobs = {
            'ci.yml': {'test': 'make test'},
            'go-ci.yml': {'go-test': 'make test'},
            'rust-ci.yml': {
                'rust-pr': 'nextest run --workspace',
                'rust-pr-windows-clippy': 'test -p parity',
                'rust-native': 'tests/test_mcp_stdio_oracle_check.py',
            },
            'rust-no-go.yml': {'frozen-neutral': 'Execute default suite with Go absent'},
            'mcp-http-native.yml': {'native-mcp': 'Complete response headers'},
            'consent-windows.yml': {'native-consent': 'Capture independent native identity'},
            'host-agent-windows.yml': {'native-agent': 'Rust host-agent reaping'},
            'campaign-smtp-native.yml': {'native-smtp': 'Actual SMTP transport'},
            'triage-native.yml': {'native-triage': 'Every retained CLI triage input'},
            'mcp-tools-native.yml': {'native-tools': 'Compiled native agent'},
            'backup-restore-native.yml': {'native': 'Execute migration CLI'},
            'frozen-http-wire-capture.yml': {'capture': 'Read interruption controls'},
            'frozen-http-aux-capture.yml': {'capture': 'Replay every auxiliary comparison'},
        }
        for workflow, targets in jobs.items():
            text = (ROOT / '.github/workflows' / workflow).read_text()
            for job, gate in targets.items():
                with self.subTest(workflow=workflow, job=job):
                    match = re.search(r'^  ' + re.escape(job) + r':\n(.*?)(?=^  [\w-]+:\n|\Z)',
                                      text, re.MULTILINE | re.DOTALL)
                    if match is None:
                        raise AssertionError(f'Missing workflow job: {workflow}/{job}')
                    body = match.group(1)
                    command = 'python3 scripts/prepare-frozen-history.py --fetch'
                    self.assertEqual(body.count(command), 1)
                    self.assertLess(body.index(command), body.index(gate))
            triggers = text.split('\njobs:', 1)[0]
            if 'paths:' in triggers:
                self.assertIn('scripts/prepare-frozen-history.py', triggers)
                self.assertIn('tests/test_prepare_frozen_history.py', triggers)


if __name__ == '__main__':
    unittest.main()
