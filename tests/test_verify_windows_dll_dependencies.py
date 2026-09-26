"""Fail-closed checks for the Windows release import contract."""

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
sys.dont_write_bytecode = True

from verify_windows_dll_dependencies import verify


class WindowsDllDependencyTests(unittest.TestCase):
    def test_system_dlls_and_windows_api_sets_are_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            system32 = Path(temporary)
            (system32 / "kernel32.dll").touch()
            (system32 / "ws2_32.dll").touch()
            verify(
                "Dump of file symeraseme.exe\n  Image has following dependencies:\n"
                "    KERNEL32.dll\n    WS2_32.dll\n"
                "    api-ms-win-core-synch-l1-2-0.dll\n",
                system32,
            )

    def test_dynamic_crt_families_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            system32 = Path(temporary)
            for name in (
                "api-ms-win-crt-runtime-l1-1-0.dll",
                "ucrtbase.dll",
                "VCRUNTIME140_1.dll",
                "MSVCP140.dll",
                "MSVCR120.dll",
                "libgcc_s_seh-1.dll",
                "libstdc++-6.dll",
            ):
                with self.subTest(name=name), self.assertRaisesRegex(ValueError, "dynamic C runtime"):
                    verify(f"    {name}\n", system32)

    def test_non_system_dll_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, "non-system DLL"):
                verify("    vendor-runtime.dll\n", Path(temporary))

    def test_unparseable_dumpbin_output_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, "no parsed DLL"):
                verify("Dump of file symeraseme.exe\n", Path(temporary))


if __name__ == "__main__":
    unittest.main()
