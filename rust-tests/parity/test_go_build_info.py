"""Synthetic rejection controls; these are not published/native acceptance."""
import hashlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

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


class RetainedReleaseControls(unittest.TestCase):
    """Mutation controls against the real validator; no Go SDK or binary execution."""

    def test_tags_are_allowlisted_with_exact_go_members(self):
        self.assertEqual(info.DEFAULT_TAG, 'v0.12.1')
        self.assertEqual(info.SUPPORTED_TAGS, ('v0.12.1', 'v0.13.0'))
        self.assertEqual(info.binary_member('v0.12.1', 'linux/amd64'), 'symeraseme')
        self.assertEqual(info.binary_member('v0.13.0', 'darwin/arm64'), 'symeraseme-go')
        self.assertEqual(info.binary_member('v0.13.0', 'windows/amd64'), 'symeraseme-go.exe')
        for tag in ('v9.9.9', 'latest', '../v0.13.0'):
            with self.subTest(tag=tag), self.assertRaises(AssertionError):
                info.release_manifest(tag)
            with self.subTest(tag=tag), self.assertRaises(AssertionError):
                info.binary_member(tag, 'linux/amd64')

    def test_pinned_v013_manifest_has_six_go_members_and_no_rust_member(self):
        document = info.release_manifest('v0.13.0')
        self.assertEqual(document['tag'], 'v0.13.0')
        self.assertEqual(document['source_revision'], '780512f17d4dd434676d62539f81550ce048f77e')
        self.assertEqual(sorted(r['native_target'] for r in document['native_archives']),
                         sorted(['darwin/amd64', 'darwin/arm64', 'linux/amd64', 'linux/arm64',
                                 'windows/amd64', 'windows/arm64']))
        for record in document['native_archives']:
            expected = 'symeraseme-go.exe' if record['native_target'].startswith('windows/') else 'symeraseme-go'
            self.assertEqual(record['binary_member'], expected)
            self.assertNotEqual(record['binary_member'], 'symeraseme')

    def test_manifest_content_mutation_is_rejected_by_pinned_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            original = info.FIXTURE_DIR / 'v0.13.0.json'
            mutated = Path(directory) / 'v0.13.0.json'
            text = original.read_text()
            for before, after in (('780512f17d4dd434676d62539f81550ce048f77e', '0' * 40),
                                  ('"binary_sha256": "d3ba', '"binary_sha256": "e3ba'),
                                  ('"binary_bytes": 18354288', '"binary_bytes": 18354289')):
                with self.subTest(change=before):
                    self.assertIn(before, text)
                    mutated.write_text(text.replace(before, after))
                    with patch.object(info, 'FIXTURE_DIR', Path(directory)), \
                            self.assertRaisesRegex(AssertionError, 'manifest changed'):
                        info.release_manifest('v0.13.0')

    def test_binary_size_hash_and_target_mutations_are_rejected(self):
        document = info.release_manifest('v0.13.0')
        record = next(r for r in document['native_archives'] if r['native_target'] == 'darwin/amd64')
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'symeraseme-go'
            with binary.open('wb') as stream:
                stream.truncate(record['binary_bytes'] - 1)
            with self.assertRaises(AssertionError):
                info.verify_release(binary, 'darwin/amd64', 'v0.13.0')
            with binary.open('wb') as stream:
                stream.truncate(record['binary_bytes'])
            with self.assertRaisesRegex(AssertionError, 'published binary changed'):
                info.verify_release(binary, 'darwin/amd64', 'v0.13.0')
            with self.assertRaisesRegex(AssertionError, 'unrecorded retained release target'):
                info.verify_release(binary, 'linux/386', 'v0.13.0')
            link = Path(directory) / 'link'
            link.symlink_to(binary)
            with self.assertRaises(AssertionError):
                info.verify_release(link, 'darwin/amd64', 'v0.13.0')

    def test_wrong_tag_or_source_revision_is_rejected_even_with_matching_bytes(self):
        document = info.release_manifest('v0.13.0')
        record = dict(next(r for r in document['native_archives'] if r['native_target'] == 'darwin/amd64'))
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'symeraseme-go'
            binary.write_bytes(b'synthetic bytes')
            record.update(binary_bytes=binary.stat().st_size,
                          binary_sha256=hashlib.sha256(b'synthetic bytes').hexdigest())
            controlled = dict(document, native_archives=[record])
            metadata = {'native_target': 'darwin/amd64', 'source_revision': '0' * 40,
                        'module': 'module-bytes'}
            with patch.object(info, 'release_manifest', return_value=controlled), \
                    patch.object(info, 'read', return_value=metadata), \
                    self.assertRaises(AssertionError):
                info.verify_release(binary, 'darwin/amd64', 'v0.13.0')
            metadata['source_revision'] = document['source_revision']
            metadata['module'] = 'changed module bytes'
            with patch.object(info, 'release_manifest', return_value=controlled), \
                    patch.object(info, 'read', return_value=metadata), \
                    self.assertRaises(AssertionError):
                info.verify_release(binary, 'darwin/amd64', 'v0.13.0')
            with self.assertRaises(AssertionError):
                info.verify_release(binary, 'darwin/amd64', 'v0.12.1')


class FetcherControls(unittest.TestCase):
    def test_invalid_destinations_and_download_bytes_fail_closed(self):
        script = Path(__file__).resolve().parents[2] / 'scripts/fetch-retained-go-release.py'
        spec = importlib.util.spec_from_file_location('retained_fetcher', script)
        if spec is None or spec.loader is None:
            raise RuntimeError('retained fetcher module cannot be loaded')
        fetcher = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fetcher)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            alias = root / 'alias'
            alias.symlink_to(root, target_is_directory=True)
            with patch.object(fetcher.urllib.request, 'urlopen') as network:
                for invalid in (Path('relative'), alias, fetcher.REPO):
                    with self.subTest(destination=str(invalid)), self.assertRaises(AssertionError):
                        fetcher.fetch(invalid, 'darwin/arm64', 'v0.13.0')
                network.assert_not_called()
            # Deliberately invalid download bytes, never native release evidence.
            with patch.object(fetcher.urllib.request, 'urlopen', return_value=io.BytesIO(b'bad')):
                with self.assertRaisesRegex(AssertionError, 'actual published archive changed'):
                    fetcher.fetch(root, 'darwin/arm64', 'v0.13.0')
            self.assertFalse((root / 'checksums.txt').exists())
            size = info.release_manifest('v0.13.0')['checksums_asset']['bytes']
            (root / 'checksums.txt').write_bytes(b'x' * size)
            with patch.object(fetcher.urllib.request, 'urlopen') as network:
                with self.assertRaises(AssertionError):
                    fetcher.fetch(root, 'darwin/arm64', 'v0.13.0')
                network.assert_not_called()


class OptimizationControls(unittest.TestCase):
    def test_integrity_controls_with_optimized_python(self):
        result = subprocess.run(
            [sys.executable, '-E', '-O', str(Path(__file__).resolve()),
             'BuildInfoControls', 'RetainedReleaseControls', 'FetcherControls'],
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Ran 11 tests', result.stderr)


if __name__ == '__main__':
    unittest.main()
