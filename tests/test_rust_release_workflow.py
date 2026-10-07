"""Release immutability guards and source-bound SBOM attestation wiring."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ReleaseWorkflowControls(unittest.TestCase):
    def test_release_guards_and_attested_sbom_manifest(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()

        def step(name):
            marker = '      - name: ' + name + '\n'
            self.assertEqual(workflow.count(marker), 1)
            start = workflow.index(marker)
            end = workflow.find('\n      - name:', start + len(marker))
            return workflow[start:end if end != -1 else len(workflow)]

        self.assertNotIn('workflow_dispatch', workflow)
        self.assertNotIn('github.event.inputs', workflow)
        self.assertIn("github.event_name == 'push' && needs.release-cli.result == 'success'", workflow)
        self.assertNotIn('--clobber', step('Upload GUI DMG to release'))
        preflight = step('Refuse to replace an existing GUI asset')
        self.assertLess(workflow.index(preflight), workflow.index('      - name: Import Developer ID certificate'))
        attestation = step('Attest the twelve SBOM sidecar digests')
        self.assertIn('subject-path: ${{ runner.temp }}/dist/sbom-checksums.txt', attestation)
        self.assertLess(workflow.index(attestation), workflow.index('      - name: Publish the prerelease'))
        publication = step('Publish the prerelease and read every asset back')
        self.assertIn('gh attestation verify "$RUNNER_TEMP/readback/sbom-checksums.txt"', publication)
        for policy in ('--signer-workflow', '--source-digest "$GITHUB_SHA"', '--source-ref "$GITHUB_REF"'):
            self.assertIn(policy, publication)

        # Execute the actual read-only preflight with a labelled API fixture, not GitHub.
        run = preflight.split('        run: |\n', 1)[1]
        script = '\n'.join(line[10:] for line in run.splitlines() if line.startswith('          '))
        self.assertTrue(script.startswith('set -euo pipefail'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gh = root / 'gh'
            gh.write_text(
                '#!/usr/bin/env python3\nimport json, os, sys\nfrom pathlib import Path\n'
                'Path(os.environ["GH_CALLS"]).write_text(json.dumps(sys.argv[1:]))\n'
                'if sys.argv[1:3] != ["release", "view"]: raise SystemExit(99)\n'
                'if os.environ["GH_VIEW_EXIT"] != "0": raise SystemExit(1)\n'
                'print(Path(os.environ["GH_FIXTURE"]).read_text())\n')
            gh.chmod(0o755)
            cases = [
                ('new-prerelease', 'v0.13.1', True, [], 0, True),
                ('historical-dmg', 'v0.13.0', True,
                 [{'name': 'Symaira-EraseMe-0.13.0-macos.dmg'}], 0, False),
                ('stable-release', 'v0.12.1', False, [], 0, False),
                ('lookup-failure', 'v0.13.1', True, [], 1, False),
            ]
            for name, tag, prerelease, assets, lookup_exit, accepted in cases:
                with self.subTest(name=name):
                    fixture = root / (name + '.json')
                    fixture.write_text(json.dumps({'isPrerelease': prerelease, 'assets': assets}))
                    calls = root / (name + '-calls.json')
                    env = {
                        'PATH': str(root) + os.pathsep + os.environ['PATH'],
                        'HOME': str(root), 'RUNNER_TEMP': str(root),
                        'RELEASE_TAG': tag, 'GITHUB_REPOSITORY': 'danieljustus/symaira-eraseme',
                        'GH_FIXTURE': str(fixture), 'GH_CALLS': str(calls),
                        'GH_VIEW_EXIT': str(lookup_exit),
                    }
                    result = subprocess.run(['bash', '-c', script], env=env, check=False,
                                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
                    self.assertEqual(result.returncode == 0, accepted, result.stderr.decode())
                    self.assertEqual(json.loads(calls.read_text())[:3], ['release', 'view', tag])


if __name__ == '__main__':
    unittest.main()
