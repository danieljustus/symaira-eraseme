#!/usr/bin/env python3
"""Derive public native fixtures without changing retained capture originals."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def changed_paths(before, after, path=()):
    if type(before) is not type(after):
        raise ValueError('privacy derivation changed JSON type')
    if isinstance(before, dict):
        if before.keys() != after.keys():
            raise ValueError('privacy derivation changed JSON keys')
        return [p for key in before for p in changed_paths(before[key], after[key], path + (key,))]
    if isinstance(before, list):
        if len(before) != len(after):
            raise ValueError('privacy derivation changed case inventory')
        return [p for i, value in enumerate(before) for p in changed_paths(value, after[i], path + (i,))]
    return [] if before == after else [path]


def derive_service(files):
    """Replace only two random grant IDs and their dependent integrity fields."""
    manifest = json.loads(files['manifest.json'])
    cases = manifest['cli_grants']['cases']
    assert [len(c['consent_records']) for c in cases] == [1, 1, 2, 1, 0, 0]
    tokens = [json.loads(files[f'cli-grant-case-{i}.stdout'])['token'] for i in (0, 2)]
    assert len(set(tokens)) == 2 and all(len(t) == 22 for t in tokens)
    replacements = {}
    for index, token in enumerate(tokens):
        public = f'public-test-grant-{index:04d}'.ljust(22, '0')
        assert len(public) == len(token) and public not in tokens
        replacements[token.encode()] = public.encode()
        old_name = 'consent_' + digest(token.encode())[:16] + '.json'
        new_name = 'consent_' + digest(public.encode())[:16] + '.json'
        replacements[old_name.encode()] = new_name.encode()

    def replace(raw, table):
        for old, new in table.items():
            raw = raw.replace(old, new)
        return raw

    result = {}
    metadata = manifest['cli_grants']['cases']
    hash_replacements = {}
    for name, raw in files.items():
        if name == 'manifest.json':
            continue
        derived = replace(raw, replacements) if name.startswith('cli-grant-') else raw
        if derived != raw:
            assert len(derived) == len(raw)
            paths = changed_paths(json.loads(raw), json.loads(derived))
            assert paths and all(p[-1] == 'token' for p in paths), 'non-token output change'
            hash_replacements[digest(raw).encode()] = digest(derived).encode()
        result[name] = derived

    derived_manifest = replace(replace(files['manifest.json'], replacements), hash_replacements)
    paths = changed_paths(manifest, json.loads(derived_manifest))
    for path in paths:
        assert path[:2] == ('cli_grants', 'cases') and isinstance(path[2], int)
        allowed = (len(path) == 5 and path[3] == 'argv' and isinstance(path[4], int))
        allowed |= (len(path) == 5 and path[3] == 'stdout' and path[4] == 'sha256')
        allowed |= (len(path) == 6 and path[3] == 'consent_records'
                    and isinstance(path[4], int) and path[5] in ('name', 'sha256'))
        assert allowed, 'non-identifier manifest change'
    assert len(derived_manifest) == len(files['manifest.json'])
    result['manifest.json'] = derived_manifest
    updated = json.loads(derived_manifest)['cli_grants']['cases']
    for i, case in enumerate(updated):
        assert case['exit_status'] == 0
        for stream in ('stdout', 'stderr'):
            raw = result[f'cli-grant-case-{i}.{stream}']
            assert case[stream] == {'bytes': len(raw), 'sha256': digest(raw)}
        for record in case['consent_records']:
            raw = result[record['file']]
            body = json.loads(raw)
            assert record['bytes'] == len(raw) and record['sha256'] == digest(raw)
            assert record['name'] == 'consent_' + digest(body['token'].encode())[:16] + '.json'
    assert all(not any(token.encode() in raw for token in tokens) for raw in result.values())
    assert all(not any(old in raw for old in replacements) for raw in result.values())
    return result, tokens


def derive_selection(source, selection_bytes, expected_sha):
    assert digest(selection_bytes) == expected_sha, 'unreviewed selection manifest'
    manifest = json.loads(selection_bytes)
    selected = manifest['selected']
    files = {}
    root = source.resolve()
    for name, metadata in selected.items():
        path = source / name
        assert not path.is_symlink() and path.resolve().is_relative_to(root)
        raw = path.read_bytes()
        assert digest(raw) == metadata['sha256'] and len(raw) == metadata['bytes']
        files[name] = raw
    result = dict(files)
    tokens = []
    directories = sorted({str(Path(name).parent) for name in files if '/services/' in name})
    assert len(directories) == 6
    for directory in directories:
        group = {Path(name).name: raw for name, raw in files.items() if str(Path(name).parent) == directory}
        derived, ids = derive_service(group)
        tokens.extend(ids)
        result.update({directory + '/' + name: raw for name, raw in derived.items()})
    assert len(set(tokens)) == 12
    assert all(not any(token.encode() in raw for token in tokens) for raw in result.values())
    changes = {}
    for name, raw in files.items():
        after = result[name]
        paths = changed_paths(json.loads(raw), json.loads(after)) if after != raw else []
        changes[name] = {**selected[name], 'original_sha256': digest(raw),
                         'sha256': digest(after), 'bytes': len(after), 'changed_json_paths': paths}
    return result, {'source_revision': manifest['source_revision'], 'runs': manifest['runs'],
                    'original_selection_sha256': expected_sha,
                    'derivation': 'two random consent IDs per target replaced with public test identifiers; dependent token filenames and grant-frame SHA256 fields updated; other bytes unchanged',
                    'selected': changes, 'native_replay_approved': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--selection', required=True, type=Path)
    parser.add_argument('--selection-sha256', required=True)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    assert args.output.resolve() != args.source.resolve()
    files, receipt = derive_selection(args.source, args.selection.read_bytes(), args.selection_sha256)
    # Validate everything before writing. Never overwrite a different existing file.
    files['privacy-derivation.json'] = (json.dumps(receipt, indent=2) + '\n').encode()
    for name, raw in files.items():
        path = args.output / name
        assert path.resolve().is_relative_to(args.output.resolve())
        if path.exists():
            assert not path.is_symlink() and path.read_bytes() == raw
    for name, raw in files.items():
        path = args.output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists():
            path.write_bytes(raw)
    selected = receipt['selected']
    print(json.dumps({'selected_files': len(selected),
                      'changed_files': sum(v['sha256'] != v['original_sha256'] for v in selected.values()),
                      'derivation_sha256': digest(files['privacy-derivation.json']),
                      'originals_unchanged': True, 'native_replay_approved': False}))


if __name__ == '__main__':
    main()
