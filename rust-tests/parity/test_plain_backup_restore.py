"""Focused controls for the disposable historical-Go restore rehearsal."""
from contextlib import closing
import sqlite3
import tempfile
from pathlib import Path
import unittest

import backup_restore_rehearsal as rehearsal


class BackupRestoreControls(unittest.TestCase):
    def test_online_backup_keeps_committed_wal_state(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'source.db'
            backup = Path(directory) / 'backup.db'
            with closing(sqlite3.connect(source)) as database:
                database.execute('PRAGMA journal_mode=WAL')
                database.execute('CREATE TABLE requests (id INTEGER PRIMARY KEY, value TEXT)')
                database.execute('INSERT INTO requests VALUES (1, "baseline")')
                database.commit()
                self.assertTrue(Path(str(source) + '-wal').exists())
                rehearsal.sqlite_backup(source, backup)
            self.assertEqual(rehearsal.gate.snapshot(source), rehearsal.gate.snapshot(backup))

    def test_baseline_gate_rejects_partial_restore_mutation(self):
        baseline = {'total': 3, 'requests': [
            {'id': 1, 'campaign_id': 'old-a'},
            {'id': 2, 'campaign_id': 'old-b'},
            {'id': 3, 'campaign_id': 'old-c'},
        ]}
        rehearsal.validate_baseline(baseline, baseline)
        partial = {'total': 3, 'requests': [dict(row) for row in baseline['requests']]}
        partial['requests'][0]['campaign_id'] = 'partial-restore-control'
        with self.assertRaisesRegex(ValueError, 'differs from the pre-Rust baseline'):
            rehearsal.validate_baseline(partial, baseline)


if __name__ == '__main__':
    unittest.main()
