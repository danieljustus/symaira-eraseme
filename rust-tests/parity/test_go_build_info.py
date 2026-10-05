"""Synthetic rejection controls; these are not published/native acceptance."""
from pathlib import Path
import tempfile
import unittest

import go_build_info as info


def encoded_string(value):
    length, prefix = len(value), bytearray()
    while length >= 128:
        prefix.append((length & 127) | 128)
        length >>= 7
    prefix.append(length)
    return bytes(prefix) + value


class BuildInfoControls(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.binary = Path(self.directory.name) / 'synthetic-metadata-control'
        self.module = (
            'path\tgithub.com/danieljustus/symaira-eraseme/cmd/symeraseme\n'
            'mod\tgithub.com/danieljustus/symaira-eraseme\tv0.12.1\n'
            'build\tCGO_ENABLED=0\nbuild\tGOOS=linux\nbuild\tGOARCH=amd64\n'
            'build\tvcs=git\nbuild\tvcs.revision=' + 'a' * 40 + '\n'
            'build\tvcs.modified=false\n'
        ).encode()
        self.header = info.MAGIC + bytes([8, 2]) + bytes(16)

    def image(self, module=None, version=b'go1.26.6'):
        framed = info.MODULE_START + (module or self.module) + info.MODULE_END
        return self.header + encoded_string(version) + encoded_string(framed)

    def reject(self, raw):
        self.binary.write_bytes(raw)
        with self.assertRaises((AssertionError, UnicodeError, KeyError, ValueError)):
            info.read(self.binary)

    def test_complete_metadata_does_not_establish_published_identity(self):
        self.binary.write_bytes(self.image())
        self.assertEqual(info.read(self.binary)['native_target'], 'linux/amd64')
        with self.assertRaises(AssertionError):
            info.verify_release(self.binary, 'linux/amd64')

    def test_truncation_alignment_pointer_format_and_length_bounds(self):
        image = self.image()
        for raw in (image[:31], image[:-1], b'x' + image,
                    info.MAGIC + bytes([8, 0]) + bytes(16) + image[32:],
                    self.header + b'\xff' * 10,
                    self.header + encoded_string(b'x' * 129)):
            with self.subTest(bytes=len(raw)):
                self.reject(raw)

    def test_native_clean_cgo_free_source_settings_required(self):
        for before, after in ((b'CGO_ENABLED=0', b'CGO_ENABLED=1'),
                              (b'vcs.modified=false', b'vcs.modified=true'),
                              (b'GOOS=linux', b'GOOS=other'),
                              (b'GOARCH=amd64', b'GOARCH=386'),
                              (b'vcs=git', b'vcs=other'),
                              (b'a' * 40, b'g' * 40)):
            with self.subTest(setting=before):
                self.reject(self.image(self.module.replace(before, after)))
        self.reject(self.image(self.module + b'build\tGOOS=linux\n'))

    def test_frame_toolchain_entrypoint_and_module_are_required(self):
        self.reject(self.image(version=b'go1.27.1'))
        self.reject(self.image().replace(info.MODULE_END, bytes(16)))
        self.reject(self.image(self.module.replace(b'/cmd/symeraseme', b'/cmd/other')))
        self.reject(self.image(self.module.replace(b'mod\tgithub.com/', b'mod\tother.com/')))

    def test_symlink_and_unrecorded_target_rejected(self):
        self.binary.write_bytes(self.image())
        alias = self.binary.with_name('symlink')
        alias.symlink_to(self.binary)
        with self.assertRaises(AssertionError):
            info.read(alias)
        with self.assertRaises(AssertionError):
            info.verify_release(self.binary, 'linux/386')


if __name__ == '__main__':
    unittest.main()
