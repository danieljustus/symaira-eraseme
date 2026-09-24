"""Offline archive contract and executable target identity checks."""

import contextlib
import io
import struct
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True

from stage_rust_release_archives import TARGETS, main as stage_main
from verify_release_archives import main as verify_archives


def executable_header(os_name: str, arch: str) -> bytes:
    if os_name == "linux":
        data = bytearray(64)
        data[:7] = b"\x7fELF\x02\x01\x01"
        data[7] = 0
        struct.pack_into("<HH", data, 16, 3, {"amd64": 62, "arm64": 183}[arch])
        struct.pack_into("<I", data, 20, 1)
        struct.pack_into("<H", data, 52, 64)
        return bytes(data) + b"linux payload"
    if os_name == "darwin":
        endian = "<"
        magic = b"\xcf\xfa\xed\xfe"
        header = magic + struct.pack(
            endian + "IIIIIII", {"amd64": 0x01000007, "arm64": 0x0100000C}[arch], 3, 2, 1, 24, 0, 0
        )
        build_version = struct.pack(endian + "IIIIII", 0x32, 24, 1, 0, 0, 0)
        return header + build_version + b"darwin payload"

    data = bytearray(0x80 + 4 + 20 + 0xF0)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", data, 0x84, {"amd64": 0x8664, "arm64": 0xAA64}[arch])
    struct.pack_into("<H", data, 0x84 + 16, 0xF0)
    struct.pack_into("<H", data, 0x84 + 20, 0x20B)
    return bytes(data) + b"windows payload"


class StageRustReleaseArchivesTests(unittest.TestCase):
    def _inputs(self, root: Path, replacements: dict[tuple[str, str], bytes] | None = None):
        inputs = root / "inputs"
        inputs.mkdir()
        args = ["--version", "0.1.0-beta.1", "--output", str(root / "staged")]
        payloads = {}
        replacements = replacements or {}
        for os_name, arch, _, _ in TARGETS:
            payload = replacements.get((os_name, arch), executable_header(os_name, arch))
            binary = inputs / f"{os_name}-{arch}.bin"
            binary.write_bytes(payload)
            payloads[(os_name, arch)] = payload
            args.extend((f"--{os_name}-{arch}", str(binary)))
        return args, payloads

    def test_six_archive_set_matches_offline_release_validator(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args, expected_payloads = self._inputs(root)
            output = root / "staged"

            self.assertEqual(stage_main(args), 0)
            verify_archives(output)

            expected_names = {
                f"symeraseme_0.1.0-beta.1_{os_name}_{arch}.{extension}"
                for os_name, arch, extension, _ in TARGETS
            }
            actual_names = {
                path.name for path in output.iterdir() if path.name.endswith((".tar.gz", ".zip"))
            }
            self.assertEqual(actual_names, expected_names)
            self.assertEqual((output / "metadata.json").read_text(), '{\n  "version": "0.1.0-beta.1"\n}\n')

            for os_name, arch, extension, binary_name in TARGETS:
                archive_path = output / f"symeraseme_0.1.0-beta.1_{os_name}_{arch}.{extension}"
                if extension == "tar.gz":
                    with tarfile.open(archive_path, "r:gz") as archive:
                        self.assertEqual(set(archive.getnames()), {binary_name, "LICENSE", "README.md"})
                        member = archive.getmember(binary_name)
                        self.assertTrue(member.mode & 0o111)
                        self.assertEqual(archive.extractfile(binary_name).read(), expected_payloads[(os_name, arch)])
                else:
                    with zipfile.ZipFile(archive_path) as archive:
                        self.assertEqual(set(archive.namelist()), {binary_name, "LICENSE", "README.md"})
                        info = archive.getinfo(binary_name)
                        self.assertEqual(info.external_attr >> 16 & 0o170000, 0o100000)
                        self.assertEqual(archive.read(binary_name), expected_payloads[(os_name, arch)])

    def test_rejects_wrong_architecture_and_operating_system(self) -> None:
        cases = (
            (("linux", "amd64"), executable_header("linux", "arm64")),
            (("linux", "amd64"), executable_header("windows", "amd64")),
        )
        for target, payload in cases:
            with self.subTest(target=target, payload=payload[:4]), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                args, _ = self._inputs(root, {target: payload})
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(stage_main(args), 1)
                self.assertFalse((root / "staged").exists())


if __name__ == "__main__":
    unittest.main()
