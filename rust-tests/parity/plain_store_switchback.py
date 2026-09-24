#!/usr/bin/env python3
"""macOS plain-store Go -> Rust -> Go regression, never release acceptance.

The caller supplies independently built artifacts. No production store, old
fallback evidence, schema guard or installed executable is changed. Linux and
Windows need their own confinement/cleanup proof before this gate supports them.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]
LINUX_SANDBOX = Path(__file__).with_name('linux_store_sandbox.py').resolve()
GUEST_LOCAL_FILESYSTEMS = {'ext4', 'xfs', 'btrfs', 'tmpfs'}
CASES = ('go-baseline', 'rust-write', 'rust-plan', 'rust-requests',
         'go-plan-after-switch', 'go-requests-after-switch')
RETAINED_GO_SHA256 = {
    # Local retained artifact and the verified v0.12.1 darwin_arm64 release executable.
    'd2cafdd118ad8c81bd29f7d165949f78dc2722d0b5b043368a0db616d4838f22',
    'b90ff3e0c16a5bfb6a9c751d79845f74983217b3f0af9d9f74faa3a255e325a3',
}
OFFICIAL_GO_V0121_SHA256 = 'b90ff3e0c16a5bfb6a9c751d79845f74983217b3f0af9d9f74faa3a255e325a3'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False,
                      default=lambda value: {'sqlite_blob_hex': value.hex()})


def save(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def same(actual, expected, message):
    require(canonical(actual) == canonical(expected), message)


def identity(path):
    require(path.is_file() and not path.is_symlink(), 'artifact must be a regular file')
    raw = path.read_bytes()
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'size': len(raw)}


def command(root, label, argv, env, timeout=30, expected_exit_code=0):
    """Retain failures and raw bytes; bound file growth and the owned process group."""
    import resource

    def bounds():
        maximum = 4 * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_FSIZE, (maximum, maximum))

    record = {'argv': list(map(str, argv)), 'cwd': str(root), 'exit_code': None,
              'timed_out': False, 'success': False,
              'expected_exit_code': expected_exit_code, 'expectation_met': False}
    try:
        with (root / (label + '.stdout')).open('xb') as out, \
                (root / (label + '.stderr')).open('xb') as err:
            child = subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.DEVNULL,
                                     stdout=out, stderr=err, start_new_session=True,
                                     preexec_fn=bounds)
            try:
                child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                record['timed_out'] = True
            finally:
                # Exactly one group cleanup, including descendants of an exited leader.
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                record['exit_code'] = child.wait(timeout=5)
        record['success'] = record['exit_code'] == 0 and not record['timed_out']
        record['expectation_met'] = (record['exit_code'] == expected_exit_code
                                     and not record['timed_out'])
    finally:
        audit = root / '.sandbox' / (label + '.audit.json')
        if audit.is_file() and not audit.is_symlink():
            record['sandbox'] = json.loads(audit.read_bytes())
        for stream in ('stdout', 'stderr'):
            path = root / (label + '.' + stream)
            if path.exists():
                record[stream] = {'path': path.name, **identity(path)}
        save(root / (label + '.json'), record)
    require(record['expectation_met'], label + ': command failed or differed from expected exit; retained raw evidence')
    return record


def retained_go_post_rust_probe(root, active, artifact, env, args, expected_diagnostic,
                                label='retained-go-post-rust-refusal'):
    """Record the retained fallback's post-Rust refusal beside the Go positive control."""
    artifact = Path(artifact).resolve(strict=True)
    artifact_identity = identity(artifact)
    require(artifact_identity['sha256'] in RETAINED_GO_SHA256,
            'retained Go artifact does not match the recorded rollback binary')
    stage = active.with_suffix('.next')
    shutil.copyfile(artifact, stage)
    stage.chmod(0o700)
    require(identity(stage) == artifact_identity, 'retained Go staging mismatch')
    os.replace(stage, active)
    result = command(root, label, sandbox_command(root, label, active, args, env), env,
                     expected_exit_code=1)
    stderr = (root / (label + '.stderr')).read_bytes()
    require(expected_diagnostic in stderr,
            'retained Go did not fail at its expected post-Rust compatibility guard')
    return {'artifact': artifact_identity, 'command': result,
            'diagnostic': expected_diagnostic.decode('utf-8', errors='replace')}


def snapshot(database):
    # This is our disposable store, not the immutable repository fixture.
    # Read live WAL and close explicitly: a sqlite connection context doesn't close.
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as db:
        tables = [r[0] for r in db.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
        state = {}
        for name in tables:
            quoted = '"' + name.replace('"', '""') + '"'
            state[name] = {
                'columns': list(db.execute('PRAGMA table_info(' + quoted + ')')),
                'rows': sorted((canonical(list(row)) for row in db.execute('SELECT * FROM ' + quoted))),
            }
        return {'user_version': db.execute('PRAGMA user_version').fetchone()[0],
                'integrity': db.execute('PRAGMA integrity_check').fetchall(),
                'schema': db.execute('SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name').fetchall(),
                'tables': state}


def without_user_version(state):
    return {key: value for key, value in state.items() if key != 'user_version'}


def sandbox(root, executable, extra_reads=()):
    if sys.platform.startswith('linux'):
        return {
            'mechanism': 'private mount, network and PID namespaces with Landlock ABI 4',
            'writable_root': str(Path(root).resolve()),
            'readonly_system_runtime': ['/usr', '/lib', '/lib64', '/proc'],
            'read_only_host_shares': 'all writable virtiofs mounts inside child namespace',
            'child_uid': os.getuid(),
            'child_gid': os.getgid(),
            'network': 'private network namespace plus Landlock TCP bind/connect denial',
            'exec': str(Path(executable).resolve()),
        }
    quote = lambda value: json.dumps(str(value))
    extra_allow = [
        '(allow file-read* (subpath ' + quote(Path(extra).resolve()) + '))'
        for extra in extra_reads
    ]
    return '\n'.join([
        '(version 1)', '(allow default)', '(deny network*)',
        '(deny file-read* (subpath ' + quote(Path.home().resolve()) + ') (subpath ' + quote(REPO) + '))',
        '(allow file-read* (subpath ' + quote(root) + '))',
        *extra_allow,
        # SQLite resolves ancestors. Permit their metadata, not their contents.
        '(allow file-read-metadata ' + ' '.join('(literal ' + quote(p) + ')' for p in root.parents) + ')',
        '(deny file-write*)',
        '(allow file-write* (subpath ' + quote(root) + ') (literal "/dev/null"))',
        '(deny process-exec)',
        '(allow process-exec (literal ' + quote(executable) + '))',
    ])


def sandbox_command(root, label, executable, args, env):
    executable = Path(executable).resolve(strict=True)
    args = list(map(str, args))
    if sys.platform == 'darwin':
        extra_reads = (env['GOROOT'],) if label == 'go-build-info' else ()
        return ['/usr/bin/sandbox-exec', '-p', sandbox(root, executable, extra_reads),
                str(executable), *args]
    require(sys.platform.startswith('linux'), 'unsupported sandbox platform')
    root = Path(root)
    require(not root.is_symlink(), 'Linux run root must not be a symlink')
    root = root.resolve(strict=True)
    mount_type = None
    for line in Path('/proc/self/mountinfo').read_text().splitlines():
        fields = line.split()
        try:
            separator = fields.index('-')
            mountpoint = fields[4].replace('\\040', ' ').replace('\\011', '\t')
            filesystem = fields[separator + 1]
        except (ValueError, IndexError):
            continue
        if root.as_posix() == mountpoint or root.as_posix().startswith(mountpoint.rstrip('/') + '/'):
            if mount_type is None or len(mountpoint) > len(mount_type[0]):
                mount_type = (mountpoint, filesystem)
    require(mount_type is not None and mount_type[1] in GUEST_LOCAL_FILESYSTEMS,
            'Linux switchback run root must be guest-local, not virtiofs')
    sandbox_dir = root / '.sandbox'
    sandbox_dir.mkdir(mode=0o700, exist_ok=True)
    config_path = sandbox_dir / (label + '.config.json')
    audit_path = sandbox_dir / (label + '.audit.json')
    config = {
        'root': str(root), 'executable': str(executable),
        'argv': [str(executable), *args], 'env': dict(env),
        'cwd': str(root), 'uid': os.getuid(), 'gid': os.getgid(),
        'audit_path': str(audit_path),
    }
    fd = os.open(config_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w', encoding='utf-8') as stream:
        json.dump(config, stream, sort_keys=True)
        stream.write('\n')
    return [sys.executable, '-I', '-S', str(LINUX_SANDBOX),
            '--launch', str(config_path)]


def create_write_probe(root, label, host_share):
    """Create a harmless marker outside the writable run root for denial checks."""
    if host_share:
        selected = None
        for line in Path('/proc/self/mountinfo').read_text().splitlines():
            fields = line.split()
            try:
                separator = fields.index('-')
                mountpoint = fields[4].replace('\\040', ' ').replace('\\011', '\t')
                filesystem = fields[separator + 1]
            except (ValueError, IndexError):
                continue
            if filesystem == 'virtiofs' and 'rw' in fields[5].split(','):
                if selected is None or len(mountpoint) > len(selected):
                    selected = mountpoint
        require(selected is not None, 'Linux sandbox requires a writable virtiofs probe mount')
        parent = Path(selected)
    else:
        parent = Path('/var/tmp')
        require(not parent.is_symlink() and parent.is_dir(),
                'Linux sandbox requires guest-local /var/tmp')
    selected_mount = None
    for line in Path('/proc/self/mountinfo').read_text().splitlines():
        fields = line.split()
        try:
            separator = fields.index('-')
            mountpoint = fields[4].replace('\\040', ' ').replace('\\011', '\t')
            filesystem = fields[separator + 1]
            options = fields[5].split(',')
        except (ValueError, IndexError):
            continue
        if (parent.as_posix() == mountpoint or
                parent.as_posix().startswith(mountpoint.rstrip('/') + '/')):
            candidate = (len(mountpoint), mountpoint, filesystem, options)
            if selected_mount is None or candidate[0] > selected_mount[0]:
                selected_mount = candidate
    require(selected_mount is not None and 'rw' in selected_mount[3],
            'write probe parent must have a writable covering mount')
    if host_share:
        require(selected_mount[2] == 'virtiofs',
                'host-share write probe must be on a writable virtiofs mount')
    else:
        require(selected_mount[2] in GUEST_LOCAL_FILESYSTEMS,
                'Landlock write probe must be on allowlisted guest-local storage')
    name = '.symeraseme-' + Path(root).name + '-' + label + ('-host' if host_share else '-landlock')
    path = parent / name
    payload = ('symeraseme disposable write-denial probe ' + name + '\n').encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
    except BaseException:
        path.unlink(missing_ok=True)
        raise
    return {'path': str(path), 'size': len(payload),
            'sha256': hashlib.sha256(payload).hexdigest(), 'host_share': host_share,
            'mountpoint': selected_mount[1], 'filesystem': selected_mount[2]}


def verify_write_probe(probe):
    path = Path(probe['path'])
    payload = path.read_bytes()
    require(len(payload) == probe['size']
            and hashlib.sha256(payload).hexdigest() == probe['sha256'],
            'outside-run-root write probe changed')
    return identity(path)


def remove_write_probe(probe):
    after = verify_write_probe(probe)
    path = Path(probe['path'])
    path.unlink()
    require(not path.exists(), 'owned outside-run-root probe was not removed')
    return {'after': after, 'removed': True}


PROBE = '''import errno,json,os,socket,subprocess,sys
result={}
for i,path in enumerate(sys.argv[1:2]):
 try:
  with open(path,'rb') as stream: stream.read(1)
 except PermissionError: result['read_denied_'+str(i)]=True
 else: result['read_denied_'+str(i)]=False
try: entries=os.scandir(sys.argv[2])
except PermissionError: result['home_directory_read_denied']=True
else:
 entries.close()
 result['home_directory_read_denied']=False
try:
 with socket.socket() as sock: sock.connect(('127.0.0.1',9))
except OSError as exc: result['network_denied']=exc.errno in (errno.EPERM,errno.EACCES)
else: result['network_denied']=False
try:
 with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as sock:
  sock.sendto(b'probe',('198.51.100.1',9))
except OSError as exc:
 result['udp_network_denied']=exc.errno in (errno.EPERM,errno.EACCES,errno.ENETUNREACH,errno.EHOSTUNREACH)
else: result['udp_network_denied']=False
try:
 with open(sys.argv[3],'r+b'): pass
except OSError as exc: result['outside_write_denied']=exc.errno in (errno.EPERM,errno.EACCES,errno.EROFS)
else: result['outside_write_denied']=False
for key,path in zip(('host_share_write_denied','guest_local_write_denied'),sys.argv[4:6]):
 try:
  with open(path,'r+b'): pass
 except OSError as exc: result[key]=exc.errno in (errno.EPERM,errno.EACCES,errno.EROFS)
 else: result[key]=False
try: subprocess.run(['/usr/bin/true'],check=True)
except PermissionError: result['child_exec_denied']=True
else: result['child_exec_denied']=False
print(json.dumps(result,sort_keys=True))
sys.exit(0 if all(result.values()) else 1)
'''


def run(go, rust, go_tool, root, retained_go=None):
    require(sys.platform == 'darwin' or sys.platform.startswith('linux'),
            'unsupported: switchback confinement is available on macOS and Linux')
    if sys.platform.startswith('linux'):
        require(os.getuid() != 0 and os.getgid() != 0,
                'Linux sandbox runner must start as an unprivileged user')
        require(platform.machine().lower() in ('aarch64', 'arm64'),
                'Linux disposable switchback requires native aarch64')
    root = Path(root)
    require(not root.is_symlink(), 'switchback run root must not be a symlink')
    root = root.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    scope = ('linux-aarch64-plain-store-disposable-runtime-only'
             if sys.platform.startswith('linux') else 'macos-plain-store-runtime-only')
    report = {'scope': scope, 'status': 'failed',
              'platform': {'system': platform.system(), 'machine': platform.machine()},
              'required_cases': list(CASES) + (['retained-go-bridge-read-four',
                                                'retained-go-bridge-create-fifth',
                                                'retained-go-bridge-read-five',
                                                'rust-bridge-read-five',
                                                'retained-go-post-rust-refusal',
                                                'retained-go-after-v1-restore'] if retained_go else []),
              'steps': [], 'schema_sequence': [],
              'database_restore_performed': False, 'publication_verified': False,
              'source_binding': 'requires caller build evidence; hashes alone are not source identity'}
    try:
        for name in ('data', 'home', 'config', 'cache', 'tmp', 'bin', 'empty-path'):
            (root / name).mkdir(mode=0o700)
        active, database = root / 'bin/symeraseme', root / 'data/symeraseme.db'
        go, rust, go_tool = (p.resolve(strict=True) for p in (go, rust, go_tool))
        retained_go = Path(retained_go).resolve(strict=True) if retained_go else None
        report['artifacts'] = {'go': identity(go), 'rust': identity(rust)}
        if retained_go:
            report['retained_go_artifact'] = identity(retained_go)
        require(identity(go)['sha256'] != identity(rust)['sha256'], 'Go and Rust artifacts must differ')
        fixture = REPO / 'tests/fixtures/event-store/golden-campaign.db'
        report['fixture'] = identity(fixture)
        report['runner'] = identity(Path(__file__))
        if sys.platform.startswith('linux'):
            report['sandbox_helper'] = identity(LINUX_SANDBOX)
        shutil.copyfile(Path(__file__), root / 'producer.py')
        shutil.copyfile(fixture, database)  # The only database copy into the active path.
        pre_rust_backup = root / 'rollback-backup/symeraseme-v1.db'
        pre_rust_backup.parent.mkdir(mode=0o700)
        with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as source, \
                closing(sqlite3.connect(pre_rust_backup)) as backup:
            source.backup(backup)
        require(snapshot(pre_rust_backup)['user_version'] == 1,
                'rollback rehearsal backup must preserve the original schema-v1 fixture')
        report['pre_rust_backup'] = identity(pre_rust_backup)
        env = {'HOME': str(root / 'home'), 'USERPROFILE': str(root / 'home'),
               'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
               'XDG_CACHE_HOME': str(root / 'cache'), 'TMPDIR': str(root / 'tmp'),
               'TEMP': str(root / 'tmp'), 'TMP': str(root / 'tmp'),
               'PATH': str(root / 'empty-path'), 'LC_ALL': 'C', 'TZ': 'UTC',
               'GOTOOLCHAIN': 'local', 'GOWORK': 'off',
               'SYMERASEME_DATA_DIR': str(database.parent), 'SYMERASEME_DB_DIR': str(database.parent),
               'SYMERASEME_ENCRYPT_DB': 'false'}
        report['environment'] = env
        go_for_info, go_tool_for_info = go, go_tool
        if sys.platform == 'darwin' or sys.platform.startswith('linux'):
            go_for_info = root / 'bin/go-candidate'
            shutil.copyfile(go, go_for_info)
            go_for_info.chmod(0o700)
        if sys.platform.startswith('linux'):
            go_tool_for_info = root / 'bin/go-tool'
            shutil.copyfile(go_tool, go_tool_for_info)
            go_tool_for_info.chmod(0o700)
        go_info_env = dict(env)
        if sys.platform.startswith('linux'):
            # A trimmed Go tool binary still reads its GOROOT at runtime.
            # The helper grants this one explicit tree read-only.
            go_info_env['GOROOT'] = str(Path(go_tool).resolve().parent.parent)
        elif sys.platform == 'darwin':
            # Hosted Go binaries are trimmed too; scope its runtime read to GOROOT.
            go_info_env['GOROOT'] = str(go_tool.parent.parent)
        command(root, 'go-build-info',
                sandbox_command(root, 'go-build-info', go_tool_for_info,
                                ['version', '-m', str(go_for_info)], go_info_env), go_info_env)
        metadata = (root / 'go-build-info.stdout').read_text()
        require('go1.26.6' in metadata.splitlines()[0].split(), 'expected artifact built with Go 1.26.6')
        arch = {'arm64': 'arm64', 'aarch64': 'arm64',
                'x86_64': 'amd64'}[platform.machine().lower()]
        goos = 'darwin' if sys.platform == 'darwin' else 'linux'
        settings = {line.strip() for line in metadata.splitlines()}
        require({'build\tCGO_ENABLED=0', 'build\tGOOS=' + goos, 'build\tGOARCH=' + arch} <= settings,
                'Go artifact must be CGO-free and native')
        try:
            command(root, 'wrapper-negative',
                    sandbox_command(root, 'wrapper-negative', '/usr/bin/false', [], env), env)
        except ValueError:
            failed = json.loads((root / 'wrapper-negative.json').read_bytes())
            require(failed['exit_code'] == 1 and failed['timed_out'] is False, 'invalid wrapper control')
        else:
            raise ValueError('wrapper masked a failing exit')
        report['wrapper_control'] = True
        python = Path(sys.base_prefix) / 'Resources/Python.app/Contents/MacOS/Python'
        python = (python if python.is_file() else Path(sys.executable)).resolve()
        negative_policy = sandbox(root, python)
        if sys.platform == 'darwin':
            negative_policy += '\n(allow file-read* (subpath ' + json.dumps(str(Path(sys.base_prefix).resolve())) + '))'
        save(root / 'sandbox-negative-policy.json', negative_policy)
        # Harmless checked-in paths only; never probe credential contents.
        probe_args = [str(REPO / 'Cargo.toml'), str(Path.home().resolve()),
                      str(REPO / 'Cargo.toml')]
        expected_controls = {'read_denied_0', 'home_directory_read_denied',
                             'network_denied', 'udp_network_denied',
                             'outside_write_denied', 'child_exec_denied'}
        host_write_probe = guest_write_probe = None
        try:
            if sys.platform.startswith('linux'):
                host_write_probe = create_write_probe(root, 'plain', host_share=True)
                guest_write_probe = create_write_probe(root, 'plain', host_share=False)
                probe_args.extend((host_write_probe['path'], guest_write_probe['path']))
                expected_controls.update(('host_share_write_denied', 'guest_local_write_denied'))
            command(root, 'sandbox-negative',
                    sandbox_command(root, 'sandbox-negative', python,
                                    ['-I', '-S', '-c', PROBE, *probe_args], env), env)
            controls = json.loads((root / 'sandbox-negative.stdout').read_bytes())
            require(set(controls) == expected_controls
                    and all(value is True for value in controls.values()), 'sandbox control failed')
        finally:
            if sys.platform.startswith('linux'):
                report['outside_write_probes'] = {}
                if host_write_probe is not None:
                    report['outside_write_probes']['host_share'] = {
                        **host_write_probe, **remove_write_probe(host_write_probe)}
                if guest_write_probe is not None:
                    report['outside_write_probes']['guest_local'] = {
                        **guest_write_probe, **remove_write_probe(guest_write_probe)}
        report['sandbox_controls'] = controls
        if sys.platform.startswith('linux'):
            audit = json.loads((root / '.sandbox/sandbox-negative.audit.json').read_bytes())
            required = ('mnt', 'net', 'pid')
            require(audit['status'] == 'running' and audit['landlock_abi'] >= 4
                    and audit['no_new_privs'] is True and audit['uid'] == os.getuid()
                    and audit['gid'] == os.getgid()
                    and all(audit['caller_namespace_ids'][name] !=
                            audit['sandbox_namespace_ids'][name] for name in required)
                    and audit['read_only_virtiofs_mounts'],
                    'Linux namespace or filesystem boundary was not enforced')
            report['sandbox_isolation'] = audit
        policy = sandbox(root, active)
        save(root / 'sandbox-policy.json', policy)
        missing_profile = root / 'home/absent-profile.enc'

        def execute(label, args, artifact=None, command_env=None):
            command_env = env if command_env is None else command_env
            step = {'id': label, 'success': False}
            report['steps'].append(step)
            if artifact is not None:
                stage = active.with_suffix('.next')
                shutil.copyfile(artifact, stage)
                stage.chmod(0o700)
                require(identity(stage) == identity(artifact), 'staged executable mismatch')
                os.replace(stage, active)
            step['installed'] = identity(active)
            try:
                step['command'] = command(root, label,
                                          sandbox_command(root, label, active, args, command_env), command_env)
            finally:
                record = root / (label + '.json')
                if record.exists():
                    step['command'] = json.loads(record.read_bytes())
            require(not (root / (label + '.stderr')).read_bytes(), label + ': unexpected stderr')
            return json.loads((root / (label + '.stdout')).read_bytes())

        initial = snapshot(database)
        report['schema_sequence'].append(initial['user_version'])
        require(initial['user_version'] == 1, 'fixture schema must start at v1')
        baseline = execute('go-baseline', ['requests', 'list', '--output', 'json'], go)
        require(type(baseline['total']) is int and baseline['total'] == 3 and len(baseline['requests']) == 3,
                'baseline must read all three requests')
        before = snapshot(database)
        save(root / 'go-baseline-state.json', before)
        report['schema_sequence'].append(before['user_version'])
        require(before['user_version'] == 2, 'current Go must migrate the baseline to v2')
        report['steps'][-1]['success'] = True
        created = execute('rust-write', ['plan', 'create', '--campaign', 'post-rust', '--max', '1',
                          '--profile', str(missing_profile), '--output', 'json'], rust)
        require(created['campaign_id'] == 'post-rust' and type(created['planned']) is int and created['planned'] == 1,
                'Rust must create one request in the new campaign')
        report['steps'][-1]['success'] = True
        plan = execute('rust-plan', ['plan', 'show', '--campaign', 'post-rust', '--output', 'json'])
        require(type(plan['total']) is int and plan['total'] == 1 and len(plan['requests']) == 1, 'Rust plan readback')
        report['steps'][-1]['success'] = True
        requests = execute('rust-requests', ['requests', 'list', '--output', 'json'])
        require(type(requests['total']) is int and requests['total'] == 4 and len(requests['requests']) == 4,
                'Rust must retain baseline requests plus its new write')
        for old in baseline['requests']:
            require(sum(canonical(old) == canonical(row) for row in requests['requests']) == 1,
                    'baseline request changed or missing')
        after = snapshot(database)
        save(root / 'rust-postwrite-state.json', after)
        for name, table in before['tables'].items():
            require(canonical(after['tables'][name]['columns']) == canonical(table['columns']), 'table shape changed')
            if name != 'sqlite_sequence':
                require(all(row in after['tables'][name]['rows'] for row in table['rows']), 'baseline row lost')
        require(len(after['tables']['campaigns']['rows']) == len(before['tables']['campaigns']['rows']) + 1,
                'campaign was not persisted')
        require(after['integrity'] == [('ok',)] and 'imap_state' in after['tables'], 'invalid SQLite state')
        report['schema_sequence'].append(after['user_version'])
        report['steps'][-1]['success'] = True
        restored_plan = execute('go-plan-after-switch', ['plan', 'show', '--campaign', 'post-rust', '--output', 'json'], go)
        same(restored_plan, plan, 'Go plan differs after switchback')
        report['steps'][-1]['success'] = True
        restored_requests = execute('go-requests-after-switch', ['requests', 'list', '--output', 'json'])
        same(restored_requests, requests, 'Go requests differ after switchback')
        final = snapshot(database)
        save(root / 'go-final-state.json', final)
        same(final, after, 'complete post-Rust SQLite state differs')
        report['schema_sequence'].append(final['user_version'])
        require(report['schema_sequence'] == [1, 2, 2, 2], 'unexpected schema sequence')
        require(not missing_profile.exists() and identity(active) == identity(go), 'final runtime identity changed')
        require(identity(fixture) == report['fixture'], 'source fixture changed')
        report['steps'][-1]['success'] = True
        if retained_go:
            require(identity(retained_go)['sha256'] == OFFICIAL_GO_V0121_SHA256,
                    'rollback bridge requires the SHA-pinned official Go v0.12.1 artifact')
            bridge_dir = root / 'rollback-bridge'
            bridge_dir.mkdir(mode=0o700)
            bridge_db = bridge_dir / 'symeraseme.db'
            with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as source, \
                    closing(sqlite3.connect(bridge_db)) as clone:
                source.backup(clone)
            source_state = snapshot(database)
            clone_state = snapshot(bridge_db)
            same(clone_state, source_state, 'SQLite backup clone differs before downgrade')
            report['rollback_bridge'] = {
                'scope': 'disposable run root only',
                'official_go_version': 'v0.12.1',
                'official_go_sha256': OFFICIAL_GO_V0121_SHA256,
                'original_database': str(database),
                'clone_database': str(bridge_db),
                'original_state_before_bridge': source_state,
                'clone_before_downgrade': clone_state,
                'rust_artifact_source_binding': 'earlier-source artifact; not exact integrated source',
            }
            require(source_state['user_version'] == 2
                    and len(source_state['tables']['removal_requests']['rows']) == 4,
                    'rollback bridge requires the intact four-request Rust schema-v2 store')
            with closing(sqlite3.connect(bridge_db)) as clone:
                clone.execute('PRAGMA user_version = 1')
                clone.commit()
            downgraded = snapshot(bridge_db)
            same(without_user_version(downgraded), without_user_version(source_state),
                 'user_version downgrade changed state beyond its pragma')
            require(downgraded['user_version'] == 1 and snapshot(database) == source_state,
                    'rollback bridge did not preserve original Rust database')
            report['rollback_bridge']['clone_after_downgrade'] = downgraded
            report['rollback_bridge']['downgrade_changed_only_user_version'] = True
            bridge_env = dict(env, SYMERASEME_DATA_DIR=str(bridge_dir),
                              SYMERASEME_DB_DIR=str(bridge_dir))
            original_rows = execute('retained-go-bridge-read-four',
                                    ['requests', 'list', '--output', 'json'],
                                    retained_go, bridge_env)
            require(original_rows['total'] == 4 and len(original_rows['requests']) == 4,
                    'official Go v0.12.1 did not read all four Rust-era requests')
            same(sorted(map(canonical, original_rows['requests'])),
                 sorted(map(canonical, requests['requests'])),
                 'official Go v0.12.1 changed or missed a Rust-era request')
            report['steps'][-1]['success'] = True
            unique_campaign = 'rollback-bridge-' + os.urandom(8).hex()
            bridge_create = execute('retained-go-bridge-create-fifth',
                                    ['plan', 'create', '--campaign', unique_campaign, '--max', '1',
                                     '--profile', str(root / 'home/bridge-absent-profile.enc'),
                                     '--output', 'json'], retained_go, bridge_env)
            require(bridge_create['campaign_id'] == unique_campaign
                    and type(bridge_create['planned']) is int and bridge_create['planned'] == 1,
                    'official Go v0.12.1 did not create one unique fifth request')
            report['steps'][-1]['success'] = True
            five_from_go = execute('retained-go-bridge-read-five',
                                   ['requests', 'list', '--output', 'json'], command_env=bridge_env)
            require(five_from_go['total'] == 5 and len(five_from_go['requests']) == 5,
                    'official Go v0.12.1 bridge write was not readable as five requests')
            same(sorted(map(canonical, original_rows['requests'])),
                 sorted(map(canonical, [row for row in five_from_go['requests']
                                        if canonical(row) in set(map(canonical, original_rows['requests']))])),
                 'bridge fifth request changed a Rust-era request')
            require(sum(canonical(row) not in set(map(canonical, original_rows['requests']))
                        for row in five_from_go['requests']) == 1,
                    'bridge must add exactly one unique request')
            report['steps'][-1]['success'] = True
            report['rollback_bridge']['go_after_write'] = snapshot(bridge_db)
            rust_five = execute('rust-bridge-read-five',
                                ['requests', 'list', '--output', 'json'], rust, bridge_env)
            same(rust_five, five_from_go, 'current Rust could not reopen/read all five bridge requests')
            rust_state = snapshot(bridge_db)
            require(rust_state['user_version'] == 2 and rust_state['integrity'] == [('ok',)],
                    'current Rust did not migrate the rollback bridge clone back to schema v2')
            report['steps'][-1]['success'] = True
            report['rollback_bridge']['rust_after_reopen'] = rust_state
            report['rollback_bridge']['rust_reopened_all_five'] = True
            require(snapshot(database) == source_state,
                    'rollback bridge altered the original Rust schema-v2 database')
            require(not (root / 'home/bridge-absent-profile.enc').exists(),
                    'bridge create unexpectedly created profile input')
            step = {'id': 'retained-go-post-rust-refusal', 'success': False,
                    'expected': 'schema-v2 refusal from the exact retained artifact'}
            report['steps'].append(step)
            try:
                step['observation'] = retained_go_post_rust_probe(
                    root, active, retained_go, env,
                    ['requests', 'list', '--output', 'json'],
                    b'user_version=2, Go port supports up to 1')
                step['success'] = True
                step['outcome'] = 'compatibility-gap-confirmed'
            finally:
                restored = active.with_suffix('.restore')
                shutil.copyfile(go, restored)
                os.replace(restored, active)
                require(identity(active) == identity(go), 'current Go positive-control restore failed')
            same(snapshot(database), after, 'retained Go rejection changed the post-Rust database')
            with closing(sqlite3.connect(pre_rust_backup.as_uri() + '?mode=ro', uri=True)) as source, \
                    closing(sqlite3.connect(database)) as restored:
                source.backup(restored)
            restored_state = snapshot(database)
            same(restored_state, initial, 'pre-Rust backup did not restore the original schema-v1 state')
            report['database_restore_performed'] = True
            report['restore_boundary'] = {
                'scope': 'disposable switchback root only',
                'restored_schema': restored_state['user_version'],
                'restored_requests': len(baseline['requests']),
                'post_rust_requests': len(requests['requests']),
                'rust_request_recovered': False}
            restored_requests = execute('retained-go-after-v1-restore',
                                        ['requests', 'list', '--output', 'json'], retained_go)
            same(restored_requests, baseline,
                 'retained Go did not read all baseline requests after restoring schema v1')
            require(restored_requests['total'] == 3
                    and all(row['id'] != 4 for row in restored_requests['requests']),
                    'restore boundary must expose the lost post-Rust request')
            report['steps'][-1]['success'] = True
            same(snapshot(database), restored_state, 'retained Go changed the restored backup')
            restored = active.with_suffix('.restore')
            shutil.copyfile(go, restored)
            os.replace(restored, active)
            require(identity(active) == identity(go), 'current Go artifact restore failed')
        expected_cases = list(CASES) + (['retained-go-bridge-read-four',
                                        'retained-go-bridge-create-fifth',
                                        'retained-go-bridge-read-five', 'rust-bridge-read-five',
                                        'retained-go-post-rust-refusal',
                                        'retained-go-after-v1-restore'] if retained_go else [])
        require([step['id'] for step in report['steps']] == expected_cases
                and all(step['success'] is True for step in report['steps']), 'incomplete case inventory')
        report['status'] = 'compatibility-gap' if retained_go else 'passed'
    finally:
        save(root / 'report.json', report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ('go', 'rust', 'go-tool', 'output-dir'):
        parser.add_argument('--' + flag, type=Path, required=True)
    parser.add_argument('--retained-go', type=Path,
                        help='execute the recorded older rollback artifact against post-Rust schema v2')
    args = parser.parse_args()
    result = run(args.go, args.rust, args.go_tool, args.output_dir, args.retained_go)
    print(json.dumps({'status': result['status'], 'scope': result['scope'],
                      'executed_cases': len(result['steps']), 'schema_sequence': result['schema_sequence']}))


if __name__ == '__main__':
    main()
