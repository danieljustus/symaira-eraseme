"""Release immutability guards and source-bound SBOM attestation wiring."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ReleaseWorkflowControls(unittest.TestCase):
    def test_reusable_store_gate_isolates_caller_concurrency(self):
        stores = (ROOT / '.github/workflows/plain-store-switchback.yml').read_text()
        concurrency = stores.split('\nconcurrency:\n', 1)[1].split('\nenv:\n', 1)[0]
        group = concurrency.split('  group: ', 1)[1].splitlines()[0]
        self.assertEqual(group, 'plain-store-switchback-${{ github.workflow }}-${{ github.ref }}')
        self.assertIn('  cancel-in-progress: true', concurrency)
        # Structural control: the same tag must not collide across callers.
        groups = [group.replace('${{ github.workflow }}', caller)
                  .replace('${{ github.ref }}', 'refs/tags/v0.14.0')
                  for caller in ('Release', 'Native store switchback')]
        self.assertNotEqual(*groups)

    def test_release_tests_the_same_six_archives_before_publication(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        archives = workflow.split('\n  release-archives:\n', 1)[1].split('\n  release-cli:\n', 1)[0]
        self.assertIn('uses: ./.github/workflows/plain-store-switchback.yml', archives)
        self.assertNotIn('uses: ./.github/workflows/rust-prerelease.yml', archives)
        publisher = workflow.split('\n  release-cli:\n', 1)[1].split('\n  release-gui:\n', 1)[0]
        self.assertIn('    needs: release-archives\n', publisher)
        self.assertIn('name: symeraseme-rust-prerelease-archives', publisher)
        self.assertNotIn('run-id:', publisher)
        self.assertNotIn('cargo ', publisher)

        stores = (ROOT / '.github/workflows/plain-store-switchback.yml').read_text()
        self.assertIn('  workflow_call: {}\n', stores)
        self.assertIn('      - .github/workflows/release.yml\n', stores)
        self.assertEqual(stores.count('uses: ./.github/workflows/rust-prerelease.yml'), 1)
        self.assertIn('    needs: release-candidate\n', stores)
        self.assertIn('          ref: ${{ github.sha }}\n', stores)
        self.assertIn('name: symeraseme-rust-prerelease-archives', stores)
        self.assertNotIn('run-id:', stores)
        for target in ('darwin, arch: arm64', 'darwin, arch: amd64',
                       'linux, arch: arm64', 'linux, arch: amd64',
                       'windows, arch: arm64', 'windows, arch: amd64'):
            self.assertIn(target, stores)
        for guard in ("sums[archive.name] == digest", "hashlib.sha256((out / t).read_bytes())",
                      "plain['status'] == 'passed'", "encrypted['status'] == 'passed'"):
            self.assertIn(guard, stores)

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
        self.assertIn('ref: ${{ github.sha }}', step('Checkout workflow source'))
        for name in ('Checkout tagged source', 'Checkout attested source'):
            self.assertIn('ref: ${{ github.sha }}', step(name))
        archives = (ROOT / '.github/workflows/rust-prerelease.yml').read_text()
        self.assertEqual(archives.count('uses: actions/checkout@'), 2)
        self.assertEqual(archives.count('ref: ${{ github.sha }}'), 2)
        self.assertIn('      - scripts/verify-release-source.sh', archives)
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

        # Use real disposable Git repositories to exercise the exact source guard.
        guard = ROOT / 'scripts/verify-release-source.sh'
        self.assertIn('bash scripts/verify-release-source.sh', step('Verify immutable release source'))
        self.assertLess(workflow.index('      - name: Verify immutable release source'),
                        workflow.index('      - name: Import Developer ID certificate'))
        for name in ('Publish the prerelease and read every asset back',
                     'Upload GUI DMG to release', 'Verify published DMG asset and record release',
                     'Download the published DMG'):
            guarded = step(name)
            self.assertIn('bash scripts/verify-release-source.sh', guarded)
            self.assertLess(guarded.index('bash scripts/verify-release-source.sh'),
                            guarded.index('gh release'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository = root / 'build'
            remote = root / 'origin.git'
            env = {'PATH': os.environ['PATH'], 'HOME': str(root), 'GIT_CONFIG_NOSYSTEM': '1',
                   'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_AUTHOR_NAME': 'Fixture',
                   'GIT_AUTHOR_EMAIL': 'fixture@example.invalid',
                   'GIT_COMMITTER_NAME': 'Fixture', 'GIT_COMMITTER_EMAIL': 'fixture@example.invalid'}

            def git(*args):
                return subprocess.run(['git', '-C', str(repository), *args], env=env, check=True,
                                      capture_output=True, text=True, timeout=5).stdout.strip()

            subprocess.run(['git', 'init', '--bare', str(remote)], env=env, check=True,
                           capture_output=True, timeout=5)
            subprocess.run(['git', 'init', str(repository)], env=env, check=True,
                           capture_output=True, timeout=5)
            git('remote', 'add', 'origin', str(remote))
            git('commit', '--allow-empty', '-m', 'event source')
            source = git('rev-parse', 'HEAD')
            git('commit', '--allow-empty', '-m', 'different source')
            moved = git('rev-parse', 'HEAD')
            git('checkout', '--detach', source)
            env.update(GITHUB_SHA=source, GITHUB_REF='refs/tags/v0.13.1',
                       RELEASE_TAG='v0.13.1', GITHUB_REF_PROTECTED='true')
            for annotated in (False, True):
                with self.subTest(annotated=annotated):
                    tag_args = ['-f', '-a', '-m', 'fixture'] if annotated else ['-f']
                    git('tag', *tag_args, 'v0.13.1', source)
                    git('push', '--force', 'origin', 'refs/tags/v0.13.1')
                    for case in ('matching', 'moved-tag', 'wrong-checkout', 'deleted-tag',
                                 'unprotected-tag', 'wrong-event-ref'):
                        with self.subTest(case=case):
                            case_env = env.copy()
                            git('checkout', '--detach', moved if case == 'wrong-checkout' else source)
                            git('tag', *tag_args, 'v0.13.1', moved if case == 'moved-tag' else source)
                            git('push', '--force', 'origin', 'refs/tags/v0.13.1')
                            if case == 'deleted-tag':
                                git('push', 'origin', ':refs/tags/v0.13.1')
                            if case == 'unprotected-tag':
                                case_env['GITHUB_REF_PROTECTED'] = 'false'
                            if case == 'wrong-event-ref':
                                case_env['GITHUB_REF'] = 'refs/tags/v0.13.0'
                            result = subprocess.run(['bash', str(guard)], cwd=repository, env=case_env,
                                                    capture_output=True, text=True, timeout=5)
                            self.assertEqual(result.returncode == 0, case == 'matching', result.stderr)
                            self.assertEqual(git('rev-parse', 'HEAD'),
                                             moved if case == 'wrong-checkout' else source)


if __name__ == '__main__':
    unittest.main()
