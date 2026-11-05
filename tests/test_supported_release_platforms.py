"""Guard the Apple Silicon and Linux CI/release boundary from #1274."""
import importlib.util
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


class SupportedReleasePlatforms(unittest.TestCase):
    def test_no_workflow_selects_a_retired_runner_or_cross_target(self):
        for workflow in (ROOT / '.github/workflows').glob('*.yml'):
            with self.subTest(workflow=workflow.name):
                self.assertNotRegex(workflow.read_text(),
                                    r'windows-(?:latest|\d[\w-]*)|macos-(?:13|15-intel)\b|'
                                    r'\b[\w]+-pc-windows-[\w]+|x86_64-apple-darwin|darwin[_-]amd64')
        for workflow in ('consent-windows.yml', 'host-agent-windows.yml'):
            self.assertFalse((ROOT / '.github/workflows' / workflow).exists())

    def test_release_stager_and_make_require_exactly_supported_targets(self):
        spec = importlib.util.spec_from_file_location('release_stager', ROOT / 'scripts/stage_rust_release_archives.py')
        assert spec is not None and spec.loader is not None
        stager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(stager)
        self.assertEqual([(os_name, arch) for os_name, arch, _, _ in stager.TARGETS],
                         [('darwin', 'arm64'), ('linux', 'amd64'), ('linux', 'arm64')])
        make = (ROOT / 'Makefile').read_text()
        targets = re.search(r'^RELEASE_TARGETS := (.+)$', make, re.M)
        assert targets is not None
        self.assertEqual(targets.group(1).split(),
                         ['darwin-arm64', 'linux-amd64', 'linux-arm64'])

    def test_supported_native_acceptance_remains_on_pull_requests(self):
        for name in ('backup-restore-native', 'campaign-smtp-native', 'email-native-trust',
                     'frozen-http-aux-capture', 'frozen-http-wire-capture', 'llm-fixture-native',
                     'mcp-http-native', 'mcp-tools-native', 'plain-store-switchback',
                     'rust-target-proof', 'triage-native', 'app-rust-native'):
            text = (ROOT / '.github/workflows' / (name + '.yml')).read_text()
            with self.subTest(workflow=name):
                self.assertIn('  pull_request:', text.split('\njobs:', 1)[0])
                self.assertIn('macos-26' if name == 'app-rust-native' else 'macos-14', text)
                if name != 'app-rust-native':
                    self.assertIn('ubuntu-24.04', text)
                    self.assertIn('ubuntu-24.04-arm', text)


if __name__ == '__main__':
    unittest.main()
