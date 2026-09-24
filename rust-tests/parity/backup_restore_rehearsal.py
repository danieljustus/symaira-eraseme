#!/usr/bin/env python3
"""Disposable v1 backup/restore rehearsal for the retained historical Go fallback."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import platform
import shutil
import sqlite3
import subprocess
import sys

import plain_store_switchback as gate

REPO = Path(__file__).resolve().parents[2]
FIXTURE = REPO / 'tests/fixtures/event-store/golden-campaign.db'
CASES = (
    'go-pre-rust-baseline',
    'pre-rust-online-backup',
    'rust-plan-create',
    'rust-postwrite-readback',
    'retained-go-on-rust-state-failure',
    'restore-pre-rust-backup',
    'retained-go-after-restore',
    'partial-restore-negative-control',
)
RUST_CAMPAIGN = 'rust016-post-rust'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sqlite_backup(source, destination):
    """Copy a consistent SQLite view, including committed WAL content."""
    source_uri = Path(source).resolve().as_uri() + '?mode=ro'
    with closing(sqlite3.connect(source_uri, uri=True)) as src, \
            closing(sqlite3.connect(destination)) as dst:
        src.backup(dst)
        dst.commit()


def validate_baseline(actual, expected):
    require(type(actual) is dict, 'Go baseline output must be a JSON object')
    require(type(actual.get('total')) is int and actual['total'] == 3,
            'restored Go state must contain three baseline requests')
    require(type(actual.get('requests')) is list and len(actual['requests']) == 3,
            'restored Go output must contain all three baseline requests')
    gate.same(actual, expected, 'restored Go output differs from the pre-Rust baseline')


def campaign_request_ids(database, campaign):
    uri = Path(database).resolve().as_uri() + '?mode=ro'
    with closing(sqlite3.connect(uri, uri=True)) as db:
        return [row[0] for row in db.execute(
            'SELECT id FROM removal_requests WHERE campaign_id = ? ORDER BY id', (campaign,))]


def invoke(root, label, executable, args, env, *, success, json_output=True):
    try:
        gate.command(root, label,
                     gate.sandbox_command(root, label, executable, args, env), env)
    except ValueError:
        pass  # Expected failures are still raw-recorded by the shared runner.
    record_path = root / (label + '.json')
    require(record_path.is_file(), label + ': shared runner did not retain a command record')
    record = json.loads(record_path.read_bytes())
    require(record['success'] is success,
            label + ': command success state did not match the rehearsal contract')
    stdout = (root / record['stdout']['path']).read_bytes()
    stderr = (root / record['stderr']['path']).read_bytes()
    if success:
        return (json.loads(stdout) if json_output else stdout), record, stdout, stderr
    return None, record, stdout, stderr


def source_identity():
    head = subprocess.run(['git', '-C', str(REPO), 'rev-parse', 'HEAD'],
                          check=True, capture_output=True, text=True, timeout=5).stdout.strip()
    status = subprocess.run(
        ['git', '-C', str(REPO), 'status', '--porcelain=v1', '--',
         'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
         'crates/symeraseme-cli', 'crates/symeraseme-core'],
        check=True, capture_output=True, text=True, timeout=5).stdout.splitlines()
    require(not status, 'Rust production build inputs have uncommitted changes')
    return {'head': head, 'production_inputs_clean': True}


def run(go, rust, go_tool, output_dir):
    require(sys.platform == 'darwin' or sys.platform.startswith('linux'),
            'unsupported: shared switchback confinement supports macOS and Linux')
    if sys.platform.startswith('linux'):
        require(os.getuid() != 0 and os.getgid() != 0,
                'Linux sandbox runner must start as an unprivileged user')
        require(platform.machine().lower() in ('aarch64', 'arm64'),
                'Linux rehearsal requires native aarch64')

    root = Path(output_dir)
    require(not root.is_symlink(), 'rehearsal root must not be a symlink')
    root = root.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = {
        'status': 'failed',
        'scope': 'disposable-v1-plain-store-backup-restore',
        'platform': {'system': platform.system(), 'machine': platform.machine()},
        'required_cases': list(CASES),
        'steps': [],
        'schema_versions': {},
        'restore_performed': False,
        'rust_created_request_absent_after_restore': False,
        'negative_control_rejected': False,
        'publication_verified': False,
        'go_artifact_provenance': {
            'role': 'caller-supplied retained Go fallback artifact',
            'path': str(go),
            'release_identity_asserted_by_runner': False,
            'note': 'The runner records the supplied bytes and embedded build metadata; filenames alone do not establish release identity.',
        },
        'rust_source': source_identity(),
    }
    try:
        for name in ('home', 'config', 'cache', 'tmp', 'empty-path', 'bin',
                     'original/data', 'restored/data', 'partial/data'):
            (root / name).mkdir(mode=0o700, parents=True)
        fixture_identity = gate.identity(FIXTURE)
        report['fixture'] = {'path': str(FIXTURE), **fixture_identity}
        go, rust, go_tool = (Path(path).resolve(strict=True) for path in (go, rust, go_tool))
        report['go_artifact_provenance']['path'] = str(go)
        staged_go, staged_rust = root / 'bin/retained-go', root / 'bin/rust-candidate'
        shutil.copyfile(go, staged_go)
        shutil.copyfile(rust, staged_rust)
        staged_go.chmod(0o700)
        staged_rust.chmod(0o700)
        go_identity, rust_identity = gate.identity(go), gate.identity(rust)
        require(gate.identity(staged_go) == go_identity
                and gate.identity(staged_rust) == rust_identity,
                'staged executable bytes differ from supplied inputs')
        report['artifacts'] = {
            'retained_go': {'supplied_path': str(go), 'identity': go_identity},
            'rust': {'supplied_path': str(rust), 'identity': rust_identity,
                     'source': report['rust_source']},
        }
        require(report['artifacts']['retained_go']['identity'] !=
                report['artifacts']['rust']['identity'], 'Go and Rust artifact bytes must differ')

        env = {
            'HOME': str(root / 'home'), 'USERPROFILE': str(root / 'home'),
            'XDG_CONFIG_HOME': str(root / 'config'),
            'XDG_DATA_HOME': str(root / 'original/data'),
            'XDG_CACHE_HOME': str(root / 'cache'),
            'TMPDIR': str(root / 'tmp'), 'TEMP': str(root / 'tmp'),
            'TMP': str(root / 'tmp'), 'PATH': str(root / 'empty-path'),
            'LC_ALL': 'C', 'TZ': 'UTC', 'GOTOOLCHAIN': 'local', 'GOWORK': 'off',
            'SYMERASEME_DATA_DIR': str(root / 'original/data'),
            'SYMERASEME_DB_DIR': str(root / 'original/data'),
            'SYMERASEME_ENCRYPT_DB': 'false',
        }
        report['environment'] = env
        go_info_env = dict(env, GOROOT=str(go_tool.parent.parent))
        invoke(root, 'go-build-info', go_tool, ['version', '-m', str(staged_go)],
               go_info_env, success=True, json_output=False)
        go_metadata = (root / 'go-build-info.stdout').read_bytes().decode('utf-8', 'replace')
        report['go_artifact_provenance']['build_metadata'] = go_metadata
        report['go_artifact_provenance']['source_revision'] = next(
            (line.split('=', 1)[1] for line in go_metadata.splitlines()
             if line.startswith('\tbuild\tvcs.revision=')), None)
        report['go_artifact_provenance']['vcs_modified'] = next(
            (line.split('=', 1)[1] for line in go_metadata.splitlines()
             if line.startswith('\tbuild\tvcs.modified=')), None)
        report['go_artifact_provenance']['source_identity_established'] = bool(
            report['go_artifact_provenance']['source_revision']
            and report['go_artifact_provenance']['vcs_modified'] == 'false')
        original_db = root / 'original/data/symeraseme.db'
        shutil.copyfile(FIXTURE, original_db)
        report['pre_rust_source_copy'] = {'sha256': gate.identity(original_db)['sha256'],
                                          'size': original_db.stat().st_size}
        before_go = gate.snapshot(original_db)
        require(before_go['user_version'] == 1, 'disposable fixture must begin at schema v1')
        require(before_go['integrity'] == [('ok',)], 'fixture SQLite integrity check failed')
        gate.save(root / 'fixture-v1-state.json', before_go)

        baseline, _, _, stderr = invoke(root, 'go-pre-rust-baseline', staged_go,
                                        ['requests', 'list', '--output', 'json'], env,
                                        success=True)
        require(not stderr, 'pre-Rust Go baseline emitted unexpected stderr')
        require(type(baseline.get('total')) is int and baseline['total'] == 3
                and type(baseline.get('requests')) is list
                and len(baseline['requests']) == 3,
                'historical Go fallback did not read the nonempty v1 baseline')
        after_baseline = gate.snapshot(original_db)
        require(after_baseline['user_version'] == 1,
                'pre-Rust Go baseline unexpectedly changed the v1 schema')
        gate.same(after_baseline, before_go, 'Go baseline read changed disposable fixture state')
        gate.save(root / 'pre-rust-state.json', after_baseline)
        report['schema_versions']['pre_rust'] = after_baseline['user_version']
        report['steps'].append({'id': CASES[0], 'success': True,
                                'request_count': baseline['total']})

        backup = root / 'pre-rust-v1-backup.db'
        sqlite_backup(original_db, backup)
        backup_state = gate.snapshot(backup)
        gate.same(backup_state, after_baseline,
                  'SQLite online backup is not a consistent pre-Rust snapshot')
        report['backup'] = {'path': backup.name, **gate.identity(backup),
                            'method': 'SQLite online backup API',
                            'source': str(original_db),
                            'state': backup_state}
        gate.save(root / 'backup-state.json', backup_state)
        report['steps'].append({'id': CASES[1], 'success': True,
                                'method': 'SQLite online backup API',
                                'schema_version': backup_state['user_version'],
                                'sha256': report['backup']['sha256']})

        created, _, _, stderr = invoke(
            root, 'rust-plan-create', staged_rust,
            ['plan', 'create', '--campaign', RUST_CAMPAIGN, '--max', '1',
             '--profile', str(root / 'home/absent-profile.enc'), '--output', 'json'],
            env, success=True)
        require(not stderr, 'Rust write emitted unexpected stderr')
        require(created.get('campaign_id') == RUST_CAMPAIGN
                and type(created.get('planned')) is int and created['planned'] == 1,
                'Rust did not perform exactly one campaign request write')
        report['rust_created_request'] = created
        report['steps'].append({'id': CASES[2], 'success': True,
                                'created_request': created})

        listed, _, _, stderr = invoke(root, 'rust-postwrite-readback', staged_rust,
                                      ['requests', 'list', '--output', 'json'], env,
                                      success=True)
        require(not stderr and listed.get('total') == 4
                and len(listed.get('requests', [])) == 4,
                'Rust post-write readback did not show three baseline plus one new request')
        post_rust = gate.snapshot(original_db)
        require(post_rust['user_version'] == 2,
                'Rust write did not reach the expected schema-v2 boundary')
        request_rows = post_rust['tables']['removal_requests']['rows']
        require(len(request_rows) == len(after_baseline['tables']['removal_requests']['rows']) + 1,
                'Rust write did not persist exactly one additional request row')
        rust_request_ids = campaign_request_ids(original_db, RUST_CAMPAIGN)
        require(len(rust_request_ids) == 1,
                'Rust campaign must own exactly one persisted request id')
        gate.save(root / 'rust-postwrite-state.json', post_rust)
        report['post_rust_state'] = post_rust
        report['rust_created_request_ids'] = rust_request_ids
        report['schema_versions']['post_rust'] = post_rust['user_version']
        report['steps'].append({'id': CASES[3], 'success': True,
                                'request_count': listed['total']})

        _, failure, stdout, stderr = invoke(
            root, 'retained-go-on-rust-state-failure', staged_go,
            ['requests', 'list', '--output', 'json'], env, success=False)
        require(failure['exit_code'] not in (None, 0) and bool(stderr),
                'retained Go fallback did not expose a real schema-v2 failure')
        report['steps'].append({
            'id': CASES[4], 'success': True, 'observed_failure': True,
            'exit_code': failure['exit_code'], 'stdout': failure['stdout'],
            'stderr': failure['stderr'],
        })

        restored_db = root / 'restored/data/symeraseme.db'
        sqlite_backup(backup, restored_db)
        report['restore_performed'] = True
        restored_state_before_go = gate.snapshot(restored_db)
        gate.same(restored_state_before_go, after_baseline,
                  'restored database differs from the saved pre-Rust state')
        require(restored_state_before_go['user_version'] == 1,
                'restore did not return the disposable database to schema v1')
        report['steps'].append({'id': CASES[5], 'success': True,
                                'method': 'SQLite online backup API',
                                'source': backup.name,
                                'destination': str(restored_db),
                                'schema_version': restored_state_before_go['user_version'],
                                'sha256': gate.identity(restored_db)['sha256']})
        restored_env = dict(env, XDG_DATA_HOME=str(root / 'restored/data'),
                            SYMERASEME_DATA_DIR=str(root / 'restored/data'),
                            SYMERASEME_DB_DIR=str(root / 'restored/data'))
        restored, _, _, stderr = invoke(root, 'retained-go-after-restore', staged_go,
                                        ['requests', 'list', '--output', 'json'],
                                        restored_env, success=True)
        require(not stderr, 'restored Go fallback emitted unexpected stderr')
        validate_baseline(restored, baseline)
        restored_state = gate.snapshot(restored_db)
        gate.same(restored_state, after_baseline,
                  'Go after restore changed the pre-Rust baseline state')
        gate.save(root / 'restored-state.json', restored_state)
        report['restored_state'] = restored_state
        report['schema_versions']['restored'] = restored_state['user_version']
        restored_rust_ids = campaign_request_ids(restored_db, RUST_CAMPAIGN)
        require(not restored_rust_ids and
                not any(RUST_CAMPAIGN in row for row in
                        restored_state['tables']['removal_requests']['rows']),
                'Rust-created request unexpectedly exists after restoring the pre-Rust backup')
        report['rust_created_request_absent_after_restore'] = True
        report['steps'].append({'id': CASES[6], 'success': True,
                                'restored_request_count': restored['total'],
                                'rust_created_request_ids': rust_request_ids,
                                'restored_rust_request_ids': restored_rust_ids,
                                'rust_created_request_absent': True})

        partial_db = root / 'partial/data/symeraseme.db'
        sqlite_backup(backup, partial_db)
        python = Path(sys.base_prefix) / 'Resources/Python.app/Contents/MacOS/Python'
        python = (python if python.is_file() else Path(sys.executable)).resolve()
        mutation = (
            'import json,sqlite3,sys; p=sys.argv[1]; db=sqlite3.connect(p); '
            'row=db.execute("SELECT id,campaign_id FROM removal_requests ORDER BY id LIMIT 1").fetchone(); '
            'assert row is not None; db.execute("UPDATE removal_requests SET campaign_id=? WHERE id=?", '
            '("partial-restore-control",row[0])); db.commit(); '
            'print(json.dumps({"request_id":row[0],"old_campaign_id":row[1],'
            '"new_campaign_id":"partial-restore-control"},sort_keys=True)); db.close()'
        )
        mutation_env = dict(env, XDG_DATA_HOME=str(root / 'partial/data'),
                            SYMERASEME_DATA_DIR=str(root / 'partial/data'),
                            SYMERASEME_DB_DIR=str(root / 'partial/data'))
        changed, _, _, stderr = invoke(
            root, 'partial-restore-mutation', python,
            ['-I', '-S', '-c', mutation, str(partial_db)], mutation_env, success=True)
        require(not stderr and changed.get('new_campaign_id') == 'partial-restore-control',
                'partial-restore negative control mutation was not exercised')
        partial, _, _, stderr = invoke(root, 'partial-restore-negative-control', staged_go,
                                       ['requests', 'list', '--output', 'json'],
                                       mutation_env, success=True)
        require(not stderr, 'partial restore Go probe unexpectedly failed before validation')
        try:
            validate_baseline(partial, baseline)
        except ValueError:
            report['negative_control_rejected'] = True
        else:
            raise ValueError('baseline verifier accepted the partial-restore negative control')
        require(gate.snapshot(partial_db)['tables']['removal_requests']['rows'] !=
                after_baseline['tables']['removal_requests']['rows'],
                'partial restore control did not change the expected SQLite state')
        report['steps'].append({'id': CASES[7], 'success': True,
                                'baseline_verifier_rejected': True,
                                'mutation': changed})

        fixture_after = {'path': str(FIXTURE), **gate.identity(FIXTURE)}
        require(fixture_after['sha256'] == fixture_identity['sha256'],
                'checked-in v1 fixture changed')
        report['fixture_after'] = fixture_after
        partial_state = gate.snapshot(partial_db)
        gate.save(root / 'partial-restore-state.json', partial_state)
        report['partial_restore_state'] = partial_state
        require([step['id'] for step in report['steps']] == list(CASES)
                and all(step['success'] is True for step in report['steps']),
                'incomplete rehearsal case inventory')
        report['status'] = 'passed'
    finally:
        gate.save(root / 'report.json', report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--go', type=Path, required=True,
                        help='retained historical Go fallback artifact')
    parser.add_argument('--rust', type=Path, required=True,
                        help='locally built Rust CLI candidate')
    parser.add_argument('--go-tool', type=Path, required=True,
                        help='Go tool used to record retained artifact build metadata')
    parser.add_argument('--output-dir', type=Path, required=True,
                        help='new disposable evidence directory; it must not exist')
    args = parser.parse_args()
    result = run(args.go, args.rust, args.go_tool, args.output_dir)
    print(json.dumps({'status': result['status'], 'scope': result['scope'],
                      'executed_cases': len(result['steps']),
                      'schema_versions': result['schema_versions'],
                      'restore_performed': result['restore_performed'],
                      'negative_control_rejected': result['negative_control_rejected']}))


if __name__ == '__main__':
    main()
