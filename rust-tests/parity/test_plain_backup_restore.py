"""Focused controls for the disposable historical-Go restore rehearsal."""
from contextlib import closing
import io
import shutil
import sqlite3
import stat
import subprocess
import tarfile
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
import warnings
import zipfile

import backup_restore_rehearsal as rehearsal


class BackupRestoreControls(unittest.TestCase):
    def test_harness_identity_binds_current_rust_control_and_metadata_reader(self):
        files = (
            'rust-tests/parity/backup_restore_rehearsal.py',
            '.github/workflows/backup-restore-native.yml',
            'rust-tests/parity/plain_store_switchback.py',
            'rust-tests/parity/linux_store_sandbox.py',
            'rust-tests/parity/windows_store_sandbox.py',
            'rust-tests/parity/prepare_backup_restore.py',
            'rust-tests/parity/src/bin/store_sandbox_control.rs',
            'rust-tests/parity/go_build_info.py',
            'rust-tests/parity/test_plain_backup_restore.py',
            'docs/rust-port/backup-restore-rehearsal.md',
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def git(*args):
                subprocess.run(['git', '-C', str(root), *args], check=True,
                               capture_output=True, timeout=30)

            git('init', '-q')
            git('config', 'user.name', 'Harness provenance control')
            git('config', 'user.email', '88160656+danieljustus@users.noreply.github.com')
            git('config', 'core.autocrlf', 'false')
            for relative in files:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(rehearsal.REPO / relative, path)
            git('add', '.')
            git('commit', '-qm', 'Current Rust harness inputs')
            with patch.object(rehearsal, 'REPO', root):
                measured = rehearsal.harness_identity()
                self.assertEqual(set(measured['committed_harness_files']), set(files))
                self.assertEqual(measured['worktree_dirty_status'], 'clean')
                for relative in files:
                    path = root / relative
                    original = path.read_bytes()
                    path.write_bytes(original + b'\nchanged harness input\n')
                    with self.subTest(path=relative), self.assertRaisesRegex(ValueError, 'committed candidate'):
                        rehearsal.harness_identity()
                    path.write_bytes(original)
                (root / 'rust-tests/parity/src/bin/store_sandbox_control.rs').unlink()
                with self.assertRaises(FileNotFoundError):
                    rehearsal.harness_identity()

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

    def test_windows_zip_verifies_exact_regular_member_without_extracting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, binary = root / 'release.zip', root / 'retained.exe'
            payload = b'released Windows Go bytes'
            binary.write_bytes(payload)
            with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as container:
                container.writestr('symeraseme.exe', payload)
                container.writestr('../must-not-extract', b'irrelevant archive member')
            evidence = rehearsal.verify_go_archive(archive, binary)
            self.assertEqual(evidence['member'], 'symeraseme.exe')
            self.assertEqual(evidence['member_size'], len(payload))
            self.assertTrue(evidence['matches_supplied_binary'])
            self.assertEqual(set(root.iterdir()), {archive, binary})
            binary.write_bytes(b'x' * len(payload))
            with self.assertRaisesRegex(ValueError, 'differ from the supplied Go binary'):
                rehearsal.verify_go_archive(archive, binary)

    def test_windows_zip_rejects_duplicate_nested_and_nonregular_members(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, binary = root / 'release.zip', root / 'retained.exe'
            binary.write_bytes(b'fixture')
            for names, mode in [(['symeraseme.exe'] * 2, stat.S_IFREG),
                                (['bin/symeraseme.exe'], stat.S_IFREG),
                                (['symeraseme.exe'], stat.S_IFLNK),
                                (['symeraseme.exe'], stat.S_IFIFO)]:
                with self.subTest(names=names, mode=mode):
                    with zipfile.ZipFile(archive, 'w') as container:
                        for name in names:
                            item = zipfile.ZipInfo(name)
                            item.create_system = 3
                            item.external_attr = (mode | 0o600) << 16
                            with warnings.catch_warnings():
                                warnings.simplefilter('ignore', UserWarning)
                                container.writestr(item, b'fixture')
                    with self.assertRaisesRegex(ValueError, 'exactly one regular'):
                        rehearsal.verify_go_archive(archive, binary)


if __name__ == '__main__':
    unittest.main()
