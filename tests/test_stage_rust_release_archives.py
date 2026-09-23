"""Contract test with tiny placeholders; it does not validate native binaries."""

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


class StageRustReleaseArchivesTests(unittest.TestCase):
    def test_six_archive_set_matches_offline_release_validator(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            inputs = root / "inputs"
            output = root / "staged"
            inputs.mkdir()
            args = ["--version", "0.1.0-beta.1", "--output", str(output)]
            expected_payloads: dict[tuple[str, str], bytes] = {}

            for os_name, arch, _, _ in TARGETS:
                payload = f"fake-{os_name}-{arch}".encode()
                binary = inputs / f"{os_name}-{arch}.bin"
                binary.write_bytes(payload)
                expected_payloads[(os_name, arch)] = payload
                args.extend((f"--{os_name}-{arch}", str(binary)))

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


if __name__ == "__main__":
    unittest.main()
