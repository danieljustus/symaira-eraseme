"""Synthetic structural control only; native evidence is checked separately."""
import copy
import importlib.util
import json
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('privacy', ROOT / 'scripts/derive_frozen_grant_privacy.py')
assert spec is not None and spec.loader is not None
privacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(privacy)


def synthetic_group(label=''):
    tokens = [name.ljust(22, '0') for name in (f'synthetic{label}-grant-first', f'synthetic{label}-grant-next')]
    assert all(len(t) == 22 for t in tokens)
    files = {}
    cases = []
    for i, ids in enumerate([[0], [0], [0, 1], [1], [], []]):
        response = {'token': tokens[0 if i == 0 else 1]} if i in (0, 2) else {'tokens': [{'token': tokens[n]} for n in ids]}
        output = json.dumps(response, separators=(',', ':')).encode()
        files[f'cli-grant-case-{i}.stdout'] = output
        files[f'cli-grant-case-{i}.stderr'] = b''
        records = []
        for j, n in enumerate(ids):
            name = f'cli-grant-case-{i}-consent-{j}.observations.json'
            raw = json.dumps({'token': tokens[n], 'command': 'execute', 'issued_at': 1, 'expires_at': 2}).encode()
            files[name] = raw
            records.append({'file': name, 'name': 'consent_' + privacy.digest(tokens[n].encode())[:16] + '.json',
                            'mode': '0600', 'bytes': len(raw), 'sha256': privacy.digest(raw)})
        cases.append({'argv': ['grant', '--revoke', tokens[0]] if i == 3 else ['grant'],
                      'exit_status': 0, 'stdout': {'bytes': len(output), 'sha256': privacy.digest(output)},
                      'stderr': {'bytes': 0, 'sha256': privacy.digest(b'')}, 'consent_records': records})
    files['manifest.json'] = json.dumps({'source_revision': 'a' * 40, 'source_files': {'unchanged': 1},
                                       'cli_grants': {'cases': cases}}, indent=2).encode()
    files['cli-schedule.stdout'] = b'{"measurement":42}\r\n'
    return files


def synthetic_selection(directory):
    source = directory / 'source'
    source.mkdir()
    selected = {}
    for target in range(6):
        service = f'linux-amd64/services/synthetic-{target}'
        for name, raw in synthetic_group(str(target)).items():
            relative = f'{service}/{name}'
            path = source / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
            selected[relative] = {'bytes': len(raw), 'sha256': privacy.digest(raw)}
    selection = json.dumps({'source_revision': 'b' * 40, 'runs': ['synthetic-run'],
                            'selected': selected}, separators=(',', ':')).encode()
    selection_path = directory / 'selection.json'
    selection_path.write_bytes(selection)
    return source, selection_path, selection, privacy.digest(selection)


def run_main(source, selection, selection_sha, output):
    with patch.object(sys, 'argv', ['derive_frozen_grant_privacy.py',
                                    '--source', str(source),
                                    '--selection', str(selection),
                                    '--selection-sha256', selection_sha,
                                    '--output', str(output)]):
        privacy.main()


class PrivacyDerivationControl(unittest.TestCase):
    def test_replaces_only_ids_and_integrity_and_rejects_corruption(self):
        original = synthetic_group()
        retained = copy.deepcopy(original)
        derived, tokens = privacy.derive_service(original)
        self.assertEqual(original, retained)
        self.assertEqual(derived['cli-schedule.stdout'], retained['cli-schedule.stdout'])
        for name in original:
            self.assertEqual(len(derived[name]), len(original[name]))
        self.assertTrue(all(token.encode() not in raw for token in tokens for raw in derived.values()))
        for name in original:
            if 'observations' in name:
                before, after = json.loads(original[name]), json.loads(derived[name])
                self.assertEqual(privacy.changed_paths(before, after), [('token',)])
        broken = copy.deepcopy(original)
        manifest = json.loads(broken['manifest.json'])
        manifest['cli_grants']['cases'][0]['stdout']['sha256'] = '0' * 64
        broken['manifest.json'] = json.dumps(manifest, indent=2).encode()
        with self.assertRaises(AssertionError):
            privacy.derive_service(broken)
        with self.assertRaises(AssertionError):
            privacy.derive_selection(Path('.'), b'{}', '0' * 64)
        with self.assertRaises(ValueError):
            privacy.changed_paths({'value': True}, {'value': 1})
        with self.assertRaises(ValueError):
            privacy.changed_paths({'cases': [1]}, {'cases': []})
        with self.assertRaises(ValueError):
            privacy.changed_paths({'source': 1}, {'other': 1})

    def test_fail_closed_selection_and_cli_guards(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, selection_path, selection_bytes, selection_sha = synthetic_selection(root)

            broken = copy.deepcopy(synthetic_group())
            manifest = json.loads(broken['manifest.json'])
            manifest['cli_grants']['cases'][0]['stdout']['sha256'] = '0' * 64
            broken['manifest.json'] = json.dumps(manifest, indent=2).encode()
            with self.assertRaises(AssertionError):
                privacy.derive_service(broken)

            with self.assertRaisesRegex(AssertionError, 'unreviewed selection manifest'):
                privacy.derive_selection(source, selection_bytes, '0' * 64)

            selected = json.loads(selection_bytes)
            first = next(iter(selected['selected']))
            (source / first).write_bytes((source / first).read_bytes() + b'corrupt')
            with self.assertRaises(AssertionError):
                privacy.derive_selection(source, selection_bytes, selection_sha)
            (source / first).write_bytes(synthetic_group('0')[Path(first).name])

            outside = root / 'outside.bin'
            outside.write_bytes(b'synthetic outside')
            link = source / 'source-link'
            link.symlink_to(outside)
            symlink_selection = json.dumps({**selected, 'selected': {
                'source-link': {'bytes': len(outside.read_bytes()), 'sha256': privacy.digest(outside.read_bytes())}}},
                separators=(',', ':')).encode()
            with self.assertRaises(AssertionError):
                privacy.derive_selection(source, symlink_selection, privacy.digest(symlink_selection))
            link.unlink()

            traversal_selection = json.dumps({**selected, 'selected': {
                '../outside.bin': {'bytes': len(outside.read_bytes()), 'sha256': privacy.digest(outside.read_bytes())}}},
                separators=(',', ':')).encode()
            with self.assertRaises(AssertionError):
                privacy.derive_selection(source, traversal_selection, privacy.digest(traversal_selection))

            with self.assertRaises(AssertionError):
                run_main(source, selection_path, selection_sha, source)

            expected, receipt = privacy.derive_selection(source, selection_bytes, selection_sha)
            valid_output = root / 'valid-output'
            run_main(source, selection_path, selection_sha, valid_output)
            self.assertEqual((valid_output / first).read_bytes(), expected[first])
            self.assertEqual(set(receipt), {'source_revision', 'runs', 'original_selection_sha256',
                                            'derivation', 'selected', 'native_replay_approved'})
            self.assertEqual(json.loads((valid_output / 'privacy-derivation.json').read_bytes()),
                             json.loads(json.dumps(receipt)))
            run_main(source, selection_path, selection_sha, valid_output)

            collision_output = root / 'collision-output'
            collision = collision_output / first
            collision.parent.mkdir(parents=True)
            collision.write_bytes(b'preserve this synthetic file')
            with self.assertRaises(AssertionError):
                run_main(source, selection_path, selection_sha, collision_output)
            self.assertEqual(collision.read_bytes(), b'preserve this synthetic file')

            symlink_output = root / 'symlink-output'
            symlink_output.mkdir()
            protected = symlink_output / 'protected-file'
            protected.write_bytes(b'preserve this synthetic target')
            linked_destination = symlink_output / first
            linked_destination.parent.mkdir(parents=True)
            linked_destination.symlink_to(protected)
            with self.assertRaises(AssertionError):
                run_main(source, selection_path, selection_sha, symlink_output)
            self.assertEqual(protected.read_bytes(), b'preserve this synthetic target')

            escaped_output = root / 'escaped-output'
            escaped_output.mkdir()
            escaped_output.joinpath('linux-amd64').symlink_to(root / 'outside-output')
            (root / 'outside-output').mkdir()
            with self.assertRaises(AssertionError):
                run_main(source, selection_path, selection_sha, escaped_output)
            self.assertEqual(list((root / 'outside-output').iterdir()), [])


if __name__ == '__main__':
    unittest.main()
