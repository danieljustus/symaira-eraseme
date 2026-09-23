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

import plain_store_switchback as gate


class SwitchbackControls(unittest.TestCase):
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


if __name__ == '__main__':
    unittest.main()
