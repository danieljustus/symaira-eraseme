"""Execute the final publication step against labelled, local API fixtures."""
import hashlib
from itertools import product
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ImmutableReleasePublication(unittest.TestCase):
    def test_complete_draft_publication_fails_closed(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        self.assertIn('--verify-tag --draft --prerelease', workflow)
        marker = '      - name: Publish complete immutable prerelease\n'
        self.assertEqual(workflow.count(marker), 1)
        self.assertGreater(workflow.index(marker), workflow.index('subject-path: ${{ runner.temp }}/dmg/*.dmg'))
        step = workflow.split(marker, 1)[1]
        script = '\n'.join(line[10:] if line else '' for line in step.split('        run: |\n', 1)[1].splitlines())
        self.assertIn('bash scripts/verify-release-source.sh', script)
        self.assertLess(script.index('sha256sum --check'), script.index('gh release edit'))
        attest = workflow.split('\n  attest-dmg:\n', 1)[1]
        self.assertIn('    needs: release-gui\n', attest)
        self.assertIn('      contents: write\n', attest)

        for case, optimize in product(('complete', 'published', 'stable', 'wrong-tag', 'missing-field',
                     'missing-asset', 'duplicate-asset', 'upload-pending', 'corrupt-asset',
                     'missing-checksum', 'duplicate-checksum', 'download-omission',
                     'malformed-json', 'lookup-error', 'api-error', 'edit-error', 'mutable-readback',
                     'wrong-tag-readback'), ('0', '1')):
            with self.subTest(case=case, optimize=optimize), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                assets = root / 'assets'
                assets.mkdir()
                archives = [f'symeraseme_0.14.1_{system}_{arch}.{extension}'
                            for system, extension in [('darwin', 'tar.gz'), ('linux', 'tar.gz'), ('windows', 'zip')]
                            for arch in ('amd64', 'arm64')]
                dmg = 'Symaira-EraseMe-0.14.1-macos.dmg'
                sidecars = [name + suffix for name in archives for suffix in ('.cdx.json', '.rust-audit.json')]
                for name in archives + [dmg] + sidecars:
                    (assets / name).write_bytes(b'labelled synthetic asset for workflow control test\n')
                for manifest, names in [('checksums.txt', archives + [dmg]), ('sbom-checksums.txt', sidecars)]:
                    (assets / manifest).write_text(''.join(
                        f'{hashlib.sha256((assets / name).read_bytes()).hexdigest()}  {name}\n' for name in names))
                payload = {'tag_name': 'v0.14.1', 'draft': True, 'prerelease': True,
                           'assets': [{'name': p.name, 'state': 'uploaded'} for p in assets.iterdir()]}
                if case == 'published':
                    payload['draft'] = False
                elif case == 'stable':
                    payload['prerelease'] = False
                elif case == 'wrong-tag':
                    payload['tag_name'] = 'v0.14.0'
                elif case == 'missing-field':
                    del payload['draft']
                elif case == 'missing-asset':
                    payload['assets'] = [a for a in payload['assets'] if a['name'] != dmg]
                elif case == 'duplicate-asset':
                    payload['assets'].append(payload['assets'][0])
                elif case == 'upload-pending':
                    payload['assets'][0]['state'] = 'starter'
                elif case == 'corrupt-asset':
                    (assets / dmg).write_bytes(b'corrupted fixture')
                elif case in ('missing-checksum', 'duplicate-checksum'):
                    manifest = assets / 'checksums.txt'
                    rows = manifest.read_text().splitlines()
                    rows = rows[:-1] if case == 'missing-checksum' else rows[:-1] + rows[:1]
                    manifest.write_text('\n'.join(rows) + '\n')
                (root / 'api-state.json').write_text(json.dumps(payload))
                (root / 'scripts').mkdir()
                # The real tag/source guard has separate disposable-git tests.
                (root / 'scripts/verify-release-source.sh').write_text('exit 0\n')
                gh = root / 'gh'
                gh.write_text('''#!/usr/bin/env python3
import json, os, shutil, sys
from pathlib import Path
root = Path(os.environ['FIXTURE_ROOT'])
case = os.environ['FIXTURE_CASE']
args = sys.argv[1:]
with (root / 'calls.jsonl').open('a') as out:
    out.write(json.dumps(args) + '\\n')
state = root / 'api-state.json'
if args == ['release', 'view', 'v0.14.1', '--repo', 'fixture/product', '--json', 'apiUrl', '--jq', '.apiUrl']:
    if case == 'lookup-error': raise SystemExit(1)
    print('https://api.github.com/repos/fixture/product/releases/123')
elif args == ['api', '--method', 'GET', 'https://api.github.com/repos/fixture/product/releases/123']:
    if case == 'api-error': raise SystemExit(1)
    print('not-json' if case == 'malformed-json' else state.read_text())
elif args == ['api', '--method', 'GET', 'repos/fixture/product/releases/tags/v0.14.1']:
    # GitHub resolves this endpoint only after the draft is published.
    if json.loads(state.read_text()).get('draft', True):
        raise SystemExit('gh: Not Found (HTTP 404)')
    print(state.read_text())
elif args[:3] == ['release', 'download', 'v0.14.1']:
    if args[3:] != ['--repo', 'fixture/product', '--dir', str(root / 'complete-release')]:
        raise SystemExit(91)
    shutil.copytree(root / 'assets', root / 'complete-release', dirs_exist_ok=True)
    if case == 'download-omission':
        (root / 'complete-release/Symaira-EraseMe-0.14.1-macos.dmg').unlink()
elif args == ['release', 'edit', 'v0.14.1', '--repo', 'fixture/product', '--draft=false', '--prerelease', '--latest=false']:
    if case == 'edit-error': raise SystemExit(1)
    data = json.loads(state.read_text())
    data.update(draft=False, prerelease=True, immutable=case != 'mutable-readback')
    if case == 'wrong-tag-readback': data['tag_name'] = 'v0.14.0'
    state.write_text(json.dumps(data))
else:
    raise SystemExit(91)
''')
                gh.chmod(0o755)
                env = {'PATH': str(root) + os.pathsep + os.environ['PATH'], 'HOME': str(root),
                       'RUNNER_TEMP': str(root), 'RELEASE_TAG': 'v0.14.1',
                       'GITHUB_REPOSITORY': 'fixture/product', 'GH_TOKEN': 'fixture-token',
                       'PYTHONOPTIMIZE': optimize,
                       'FIXTURE_ROOT': str(root), 'FIXTURE_CASE': case}
                result = subprocess.run(['bash', '-c', script], cwd=root, env=env,
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode == 0, case == 'complete', result.stderr)
                calls = [json.loads(line) for line in (root / 'calls.jsonl').read_text().splitlines()]
                edits = [args for args in calls if args[:2] == ['release', 'edit']]
                self.assertEqual(len(edits), int(case in ('complete', 'edit-error', 'mutable-readback', 'wrong-tag-readback')))
                self.assertTrue(all(args[:2] not in (['release', 'upload'], ['release', 'delete']) for args in calls))


if __name__ == '__main__':
    unittest.main()
