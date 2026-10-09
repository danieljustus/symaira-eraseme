"""Synthetic structural control only; native evidence is checked separately."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('privacy', ROOT / 'scripts/derive_frozen_grant_privacy.py')
assert spec is not None and spec.loader is not None
privacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(privacy)


def synthetic_group():
    tokens = [name.ljust(22, '0') for name in ('synthetic-grant-first', 'synthetic-grant-next')]
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


if __name__ == '__main__':
    unittest.main()
