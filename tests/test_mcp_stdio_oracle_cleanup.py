#!/usr/bin/env python3
"""Bounded cleanup policy controls, with real Windows no-delete-sharing handles."""
import ctypes
import importlib.util
import platform
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "rust-tests/parity/oracle/mcp-stdio-mutations/generate.py"
spec = importlib.util.spec_from_file_location("mcp_stdio_cleanup", GENERATOR)
assert spec is not None and spec.loader is not None
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)


def sharing_error(code=32):
    error = PermissionError("synthetic cleanup-policy control")
    setattr(error, "winerror", code)
    return error


class CleanupPolicyTests(unittest.TestCase):
    def test_transient_windows_sharing_violation_is_retried(self):
        directory = oracle.OracleTemporaryDirectory()
        try:
            with patch.object(oracle.sys, "platform", "win32"), \
                    patch.object(tempfile.TemporaryDirectory, "cleanup",
                                 side_effect=[sharing_error(), None]) as cleanup:
                directory.cleanup()
                self.assertEqual(cleanup.call_count, 2)
        finally:
            directory.cleanup()

    def test_non_sharing_windows_errors_are_not_retried(self):
        for code in (5, 33):
            with self.subTest(winerror=code):
                directory = oracle.OracleTemporaryDirectory()
                try:
                    error = sharing_error(code)
                    with patch.object(oracle.sys, "platform", "win32"), \
                            patch.object(tempfile.TemporaryDirectory, "cleanup",
                                         side_effect=error) as cleanup:
                        with self.assertRaises(PermissionError) as raised:
                            directory.cleanup()
                        self.assertIs(raised.exception, error)
                        self.assertEqual(cleanup.call_count, 1)
                finally:
                    directory.cleanup()

    def test_non_windows_sharing_error_is_not_retried(self):
        directory = oracle.OracleTemporaryDirectory()
        try:
            error = sharing_error()
            with patch.object(oracle.sys, "platform", "darwin"), \
                    patch.object(tempfile.TemporaryDirectory, "cleanup",
                                 side_effect=error) as cleanup:
                with self.assertRaises(PermissionError) as raised:
                    directory.cleanup()
                self.assertIs(raised.exception, error)
                self.assertEqual(cleanup.call_count, 1)
        finally:
            directory.cleanup()

    def test_persistent_sharing_error_is_raised_at_deadline(self):
        directory = oracle.OracleTemporaryDirectory()
        try:
            error = sharing_error()
            with patch.object(oracle.sys, "platform", "win32"), \
                    patch.object(tempfile.TemporaryDirectory, "cleanup",
                                 side_effect=error) as cleanup, \
                    patch.object(oracle.time, "monotonic", side_effect=[0.0, 5.0]), \
                    patch.object(oracle.time, "sleep") as sleep:
                with self.assertRaises(PermissionError) as raised:
                    directory.cleanup()
                self.assertIs(raised.exception, error)
                self.assertEqual(cleanup.call_count, 1)
                sleep.assert_not_called()
        finally:
            directory.cleanup()


@unittest.skipUnless(sys.platform == "win32", "requires real Windows sharing semantics")
class NativeWindowsCleanupTests(unittest.TestCase):
    def lock_owned_file(self, directory):
        from ctypes import wintypes
        path = Path(directory.name) / "symeraseme-go.exe"
        path.write_bytes(b"owned native sharing-class control, not an executable")
        kernel = getattr(ctypes, "WinDLL")("kernel32", use_last_error=True)
        create = kernel.CreateFileW
        create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                           wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
        create.restype = wintypes.HANDLE
        close = kernel.CloseHandle
        close.argtypes = [wintypes.HANDLE]
        close.restype = wintypes.BOOL
        # Read and write sharing are permitted; deletion is explicitly not.
        handle = create(str(path), 0x80000000, 0x1 | 0x2, None, 3, 0x80, None)
        if handle == wintypes.HANDLE(-1).value:
            raise getattr(ctypes, "WinError")(getattr(ctypes, "get_last_error")())
        return path, handle, close

    def test_native_transient_no_delete_share_lock_is_recovered(self):
        directory = oracle.OracleTemporaryDirectory()
        path, handle, close = self.lock_owned_file(directory)
        observed = []
        original = tempfile.TemporaryDirectory.cleanup

        def release_after_real_failure(owned):
            nonlocal handle
            try:
                original(owned)
            except OSError as error:
                observed.append(getattr(error, "winerror", None))
                self.assertEqual(getattr(error, "winerror", None), 32)
                self.assertTrue(close(handle))
                handle = None
                raise

        try:
            with patch.object(tempfile.TemporaryDirectory, "cleanup",
                              release_after_real_failure):
                directory.cleanup()
            self.assertEqual(observed, [32])
            self.assertFalse(path.exists())
            self.assertFalse(Path(directory.name).exists())
        finally:
            if handle is not None:
                self.assertTrue(close(handle))
            directory.cleanup()

    def test_native_persistent_no_delete_share_lock_fails_bounded(self):
        directory = oracle.OracleTemporaryDirectory()
        path, handle, close = self.lock_owned_file(directory)
        started = time.monotonic()
        try:
            with self.assertRaises(OSError) as raised:
                directory.cleanup(timeout=0.15)
            self.assertEqual(getattr(raised.exception, "winerror", None), 32)
            self.assertLess(time.monotonic() - started, 2.0)
            self.assertTrue(path.is_file(), "a persistent lock must remain visible, not be ignored")
        finally:
            self.assertTrue(close(handle))
            directory.cleanup()
        self.assertFalse(Path(directory.name).exists())


if __name__ == "__main__":
    print(f"Cleanup control host: {platform.system()}/{platform.machine()}", flush=True)
    unittest.main(verbosity=2)
