"""Harness rejection/cleanup tests, not substitutes for real CLI execution."""
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import plain_store_switchback as gate


class SwitchbackControls(unittest.TestCase):
    def test_bridge_cases_are_limited_to_official_release_pin(self):
        self.assertEqual(gate.retained_cases(gate.OFFICIAL_GO_V0121_SHA256),
                         gate.BRIDGE_CASES + gate.ROLLBACK_CASES)
        self.assertEqual(gate.retained_cases(
            'd2cafdd118ad8c81bd29f7d165949f78dc2722d0b5b043368a0db616d4838f22'),
            gate.ROLLBACK_CASES)

    def test_expected_refusal_retains_nonzero_exit_as_a_negative_control(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = gate.command(
                root, 'expected-refusal',
                [sys.executable, '-I', '-S', '-c',
                 'import sys; print("unsupported schema", file=sys.stderr); sys.exit(1)'],
                {'HOME': str(root), 'PATH': ''}, expected_exit_code=1)
            self.assertFalse(result['success'])
            self.assertTrue(result['expectation_met'])
            self.assertEqual(result['exit_code'], 1)
            self.assertEqual((root / 'expected-refusal.stderr').read_bytes(),
                             b'unsupported schema\n')

    @unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox Go runtime control')
    def test_go_build_info_reads_only_explicit_goroot(self):
        go_tool = Path(subprocess.check_output(['which', 'go'], text=True).strip()).resolve()
        goroot = subprocess.check_output([str(go_tool), 'env', 'GOROOT'], text=True).strip()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            env = {'HOME': str(root), 'PATH': '', 'GOROOT': goroot}
            gate.command(root, 'go-build-info',
                         gate.sandbox_command(root, 'go-build-info', go_tool,
                                              ['version', '-m', str(go_tool)], env), env)
            metadata = (root / 'go-build-info.stdout').read_text()
            self.assertIn('\tpath\tcmd/go', metadata)
            self.assertIn('\tbuild\tGOOS=darwin', metadata)
            policy = gate.sandbox(root, go_tool, (goroot,))
            self.assertIn('(allow file-read* (subpath ' + json.dumps(goroot) + '))', policy)
            self.assertIn('(deny network*)', policy)

    def test_nested_run_allows_sqlite_but_denies_protected_data(self):
        python = Path(sys.executable).resolve()
        app = Path(sys.base_prefix) / 'Resources/Python.app/Contents/MacOS/Python'
        if app.is_file():
            python = app.resolve()
        source = '''import os,sqlite3,sys
from pathlib import Path
root,home,repo=map(Path,sys.argv[1:4])
root.parent.stat()
with sqlite3.connect(root/'owned.db') as db:
 db.execute('CREATE TABLE items(value INTEGER)')
 db.execute('INSERT INTO items VALUES(42)')
 assert db.execute('SELECT value FROM items').fetchall()==[(42,)]
for path in (home/'private',repo/'private'):
 try: path.read_bytes()
 except PermissionError: pass
 else: raise AssertionError('protected file is readable')
try:
 with open(repo/'private','r+b'): pass
except OSError: pass
else: raise AssertionError('protected file is writable')
for path in map(Path,sys.argv[4:]):
 try:
  with open(path,'r+b'): pass
 except OSError: pass
 else: raise AssertionError('outside run-root file is writable')
try: entries=os.scandir(home)
except PermissionError: pass
else:
 entries.close()
 raise AssertionError('protected directory is readable')
'''
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            home, repo = base / 'operator-home', base / 'checkout'
            for parent in (home, repo):
                parent.mkdir()
                (parent / 'private').write_text('synthetic sentinel, not operator data')
            for parent in (base, home, repo):
                with self.subTest(parent=parent.name):
                    root = parent / 'run'
                    root.mkdir()
                    probes = []
                    args = ['-I', '-S', '-c', source, str(root), str(home), str(repo)]
                    env = {'HOME': str(root), 'PATH': '', 'LC_ALL': 'C'}
                    try:
                        if sys.platform.startswith('linux'):
                            probes.append(gate.create_write_probe(base, 'control-' + parent.name,
                                                                  host_share=True))
                            probes.append(gate.create_write_probe(base, 'control-' + parent.name,
                                                                  host_share=False))
                            args.extend(probe['path'] for probe in probes)
                        with patch.object(Path, 'home', return_value=home), patch.object(gate, 'REPO', repo):
                            gate.command(root, 'owned',
                                         gate.sandbox_command(root, 'owned', python,
                                                              args,
                                                              env), env)
                    except ValueError:
                        self.fail((root / 'owned.stderr').read_text())
                    finally:
                        for probe in probes:
                            gate.remove_write_probe(probe)

    def test_typed_comparison_rejects_semantic_mutations(self):
        gate.same({'a': None, 'b': [1, 2]}, {'b': [1, 2], 'a': None}, 'positive')
        for actual, expected in [(True, 1), (1.0, 1), ({}, {'a': None}),
                                 ([2, 1], [1, 2]), ((1 << 60) + 1, 1 << 60)]:
            with self.subTest(actual=actual), self.assertRaisesRegex(ValueError, 'mutation'):
                gate.same(actual, expected, 'mutation')

    def test_complete_sqlite_comparison_rejects_null_empty_and_blob_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / 'test.db'
            with closing(sqlite3.connect(database)) as connection:
                connection.executescript("CREATE TABLE items (id INTEGER, value TEXT, payload BLOB);"
                                         "INSERT INTO items VALUES(1, NULL, x'00');")
            baseline = gate.snapshot(database)
            gate.same(gate.snapshot(database), baseline, 'positive')
            for sql in ("UPDATE items SET value=''", "UPDATE items SET payload='00'", 'DELETE FROM items'):
                with closing(sqlite3.connect(database)) as connection:
                    connection.executescript("DELETE FROM items; INSERT INTO items VALUES(1, NULL, x'00');")
                    gate.same(gate.snapshot(database), baseline, 'reset each mutation independently')
                    connection.execute(sql)
                    connection.commit()
                with self.subTest(sql=sql), self.assertRaisesRegex(ValueError, 'changed state'):
                    gate.same(gate.snapshot(database), baseline, 'changed state')

    def test_bridge_comparison_allows_only_user_version_change(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / 'bridge.db'
            with closing(sqlite3.connect(database)) as connection:
                connection.executescript('CREATE TABLE requests (id INTEGER, value TEXT);'
                                         "INSERT INTO requests VALUES(1, 'kept');"
                                         'PRAGMA user_version = 2;')
            original = gate.snapshot(database)
            downgraded = dict(original, user_version=1)
            gate.same(gate.without_user_version(downgraded),
                      gate.without_user_version(original), 'user_version-only downgrade')
            changed = dict(downgraded)
            changed['tables'] = dict(downgraded['tables'])
            changed['tables']['requests'] = dict(downgraded['tables']['requests'])
            changed['tables']['requests']['rows'] = ['[2,"changed"]']
            with self.assertRaisesRegex(ValueError, 'changed beyond its pragma'):
                gate.same(gate.without_user_version(changed),
                          gate.without_user_version(original),
                          'clone row mutation changed beyond its pragma')

    def test_identical_binaries_and_existing_output_fail_before_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            go, rust = root / 'go', root / 'rust'
            go.write_bytes(b'not an executable; must never be launched')
            rust.write_bytes(go.read_bytes())
            output = root / 'evidence'
            with self.assertRaisesRegex(ValueError, 'artifacts must differ'):
                gate.run(go, rust, Path(sys.executable), output)
            report_bytes = (output / 'report.json').read_bytes()
            report = json.loads(report_bytes)
            self.assertEqual(report['status'], 'failed')
            self.assertEqual(report['steps'], [])
            with self.assertRaises(FileExistsError):
                gate.run(go, rust, Path(sys.executable), output)
            self.assertEqual((output / 'report.json').read_bytes(), report_bytes)

    def test_failed_timeout_and_output_limit_commands_keep_raw_reports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = {'HOME': str(root), 'PATH': '', 'LC_ALL': 'C'}
            cases = [('failed', 'import sys;print("failure");sys.exit(7)', 5, False),
                     ('timeout', 'import time;time.sleep(30)', 0.2, True),
                     ('limit', 'import sys;sys.stdout.buffer.write(b"x"*(8*1024*1024))', 5, False)]
            for label, source, timeout, timed_out in cases:
                with self.subTest(label=label), self.assertRaisesRegex(ValueError, 'command failed'):
                    gate.command(root, label, [sys.executable, '-I', '-S', '-c', source], env, timeout)
                result = json.loads((root / (label + '.json')).read_bytes())
                self.assertIs(result['success'], False)
                self.assertIs(result['timed_out'], timed_out)
                self.assertNotEqual(result['exit_code'], 0)
                for stream in ('stdout', 'stderr'):
                    raw = root / result[stream]['path']
                    self.assertEqual(gate.identity(raw)['sha256'], result[stream]['sha256'])
            self.assertEqual((root / 'failed.stdout').read_bytes(), b'failure\n')
            self.assertLessEqual((root / 'limit.stdout').stat().st_size, 4 * 1024 * 1024)

    def test_exited_parent_cannot_leave_a_running_descendant(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = 'import os,time\npid=os.fork()\nif pid==0: time.sleep(30)\nelse: print(pid,flush=True)'
            start = time.monotonic()
            gate.command(root, 'descendant', [sys.executable, '-I', '-S', '-c', source], {'PATH': ''})
            self.assertLess(time.monotonic() - start, 5)
            child = int((root / 'descendant.stdout').read_bytes())
            result = subprocess.run(['/bin/ps', '-p', str(child), '-o', 'stat='],
                                    capture_output=True, text=True, timeout=5, check=False)
            self.assertTrue(result.returncode == 1 or result.stdout.strip().startswith('Z'),
                            'descendant remains running: ' + result.stdout.strip())

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux namespace helper control')
    def test_linux_helper_fails_closed_without_private_namespaces(self):
        namespace_ids = {name: os.stat('/proc/self/ns/' + name).st_ino
                         for name in ('mnt', 'net', 'pid')}
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'config.json'
            config.write_text(json.dumps({'parent_namespace': namespace_ids}))
            before = Path('/proc/self/mountinfo').read_bytes()
            result = subprocess.run(
                ['/usr/bin/sudo', '-n', '/usr/bin/python3', '-I', '-S',
                 str(gate.LINUX_SANDBOX), '--inside', str(config)],
                capture_output=True, timeout=5, check=False)
            after = Path('/proc/self/mountinfo').read_bytes()
            self.assertEqual(result.returncode, 125)
            self.assertIn(b'required private mount, network and PID namespaces are absent',
                          result.stderr)
            self.assertEqual(after, before, 'helper changed mount state without namespace isolation')


if __name__ == '__main__':
    unittest.main()
