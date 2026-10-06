"""Harness rejection/cleanup tests, not substitutes for real CLI execution."""
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import plain_store_switchback as gate


class SwitchbackControls(unittest.TestCase):
    @unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox Go runtime control')
    def test_go_build_info_reads_only_explicit_goroot(self):
        if shutil.which('go') is None:
            retained = os.environ['SYMERASEME_ROLLBACK_GO_BINARY']
            architecture = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'amd64'}[gate.platform.machine().lower()]
            gate.go_build_info.verify_release(retained, 'darwin/' + architecture)
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                artifact = root / 'retained-go'
                shutil.copyfile(retained, artifact)
                helper = root / 'go_build_info.py'
                shutil.copyfile(Path(gate.go_build_info.__file__), helper)
                python = Path(sys.executable).resolve()
                env = {'HOME': str(root), 'PATH': '', 'PYTHONDONTWRITEBYTECODE': '1'}
                code = 'import importlib.util,sys; s=importlib.util.spec_from_file_location("info",sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); print(m.read(sys.argv[2])["source_revision"])'
                gate.command(root, 'retained-go-metadata', gate.sandbox_command(root, 'retained-go-metadata', python,
                             ['-c', code, str(helper), str(artifact)], env), env)
                self.assertEqual((root / 'retained-go-metadata.stdout').read_text().strip(),
                                 '240bf67cefa05e643e32611a02e6e7ed87a033ea')
                policy = gate.sandbox(root, python)
                self.assertIn('(deny network*)', policy)
                self.assertNotIn('GOROOT', policy)
            return
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
for path in (Path(arg.split('=',1)[1]) for arg in sys.argv[4:]):
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
                            labelled = gate.linux_write_probes(base, 'control-' + parent.name)
                            probes.extend(labelled.values())
                            args.extend(key + '=' + probe['path'] for key, probe in labelled.items())
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
