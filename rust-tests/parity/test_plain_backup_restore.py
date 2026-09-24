"""Focused controls for the disposable historical-Go restore rehearsal."""
from contextlib import closing
import io
import sqlite3
import tarfile
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

    def test_baseline_gate_rejects_missing_restored_request(self):
        baseline = {'total': 3, 'requests': [
            {'id': 1, 'campaign_id': 'old-a'},
            {'id': 2, 'campaign_id': 'old-b'},
            {'id': 3, 'campaign_id': 'old-c'},
        ]}
        rehearsal.validate_baseline(baseline, baseline)
        partial = {'total': 2, 'requests': baseline['requests'][1:]}
        with self.assertRaisesRegex(ValueError, 'three baseline requests'):
            rehearsal.validate_baseline(partial, baseline)

    def test_archive_must_contain_the_supplied_go_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / 'release.tar.gz'
            binary = root / 'symeraseme'
            payload = b'released Go fallback bytes'
            binary.write_bytes(payload)
            member = tarfile.TarInfo('symeraseme')
            member.size = len(payload)
            with tarfile.open(archive, mode='w:gz') as tar:
                tar.addfile(member, io.BytesIO(payload))
            evidence = rehearsal.verify_go_archive(archive, binary)
            self.assertTrue(evidence['matches_supplied_binary'])
            self.assertEqual(evidence['member_size'], len(payload))
            binary.write_bytes(b'different executable')
            with self.assertRaisesRegex(ValueError, 'differ from the supplied Go binary'):
                rehearsal.verify_go_archive(archive, binary)


if __name__ == '__main__':
    unittest.main()
