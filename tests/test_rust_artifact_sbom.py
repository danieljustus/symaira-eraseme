"""Artifact-bound Rust dependency inventory controls and release readback."""
import copy
import io
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import tarfile
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import rust_artifact_sbom as sbom
import verify_rust_artifact_sbom as readback

INVENTORY = {'packages': [
    {'name': 'symeraseme-cli', 'version': '0.13.0', 'source': 'local', 'root': True, 'dependencies': [1]},
    {'name': 'dependency', 'version': '1.2.3', 'source': 'registry'},
]}
SOURCE_REVISION = 'a' * 40
ARCHIVE_FIXTURE_SHA256 = '75c1329b4c56a114e00a0a212a3b057fd3c255ed6bc4f89fcd9699ade08ecf67'
RUST_FIXTURE_SHA256 = 'c17e279d7bc91a54b9e97153c4b0b11139b97ce590b8b06c1a733fbb8b91ad67'


class ArtifactSbomControls(unittest.TestCase):
    def test_embedded_graph_rejects_cycle_roots_bad_indexes_and_unreachable_packages(self):
        cycle = copy.deepcopy(INVENTORY)
        cycle['packages'][1]['dependencies'] = [0]
        roots = copy.deepcopy(INVENTORY)
        roots['packages'][1]['root'] = True
        index = copy.deepcopy(INVENTORY)
        index['packages'][0]['dependencies'] = [99]
        boolean = copy.deepcopy(INVENTORY)
        boolean['packages'][0]['dependencies'] = [True]
        unreachable = copy.deepcopy(INVENTORY)
        unreachable['packages'][0]['dependencies'] = []
        root_dependency = {'packages': [
            {'name': 'root', 'version': '1', 'source': 'local', 'root': True, 'dependencies': [1]},
            {'name': 'child', 'version': '1', 'source': 'local'},
            {'name': 'orphan', 'version': '1', 'source': 'local', 'dependencies': [0]},
        ]}
        with self.assertRaisesRegex(ValueError, 'cycle'):
            sbom.validate_inventory(cycle)
        with self.assertRaisesRegex(ValueError, 'exactly one root'):
            sbom.validate_inventory(roots)
        with self.assertRaisesRegex(ValueError, 'out of range'):
            sbom.validate_inventory(index)
        with self.assertRaisesRegex(ValueError, 'out of range'):
            sbom.validate_inventory(boolean)
        with self.assertRaisesRegex(ValueError, 'unreachable'):
            sbom.validate_inventory(unreachable)
        with self.assertRaisesRegex(ValueError, 'root package cannot be a dependency'):
            sbom.validate_inventory(root_dependency)

    def test_archive_and_rust_hashes_bind_only_cargo_components(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, binary = root / 'release.tar.gz', root / 'rust'
            archive.write_bytes(b'archive-fixture-v1')
            binary.write_bytes(b'rust-fixture-v1')
            document = sbom.document(archive, binary, INVENTORY, SOURCE_REVISION, ('linux', 'amd64'))
            self.assertEqual(sbom.digest(archive), ARCHIVE_FIXTURE_SHA256)
            self.assertEqual(document['metadata']['component']['hashes'][0]['content'], ARCHIVE_FIXTURE_SHA256)
            rust = next(item for item in document['components'] if item['bom-ref'] == 'file:symeraseme')
            self.assertEqual(rust['hashes'][0]['content'], RUST_FIXTURE_SHA256)
            self.assertEqual(document['metadata']['properties'][0]['value'], SOURCE_REVISION)
            self.assertTrue(any(item.get('purl') == 'pkg:cargo/dependency@1.2.3'
                                for item in document['components']))
            self.assertFalse(any(item.get('bom-ref', '').startswith('file:symeraseme-go')
                                 or item.get('purl', '').startswith('pkg:golang/')
                                 for item in document['components']))
            refs = {item['bom-ref'] for item in document['components']} | {
                document['metadata']['component']['bom-ref']}
            for edge in document['dependencies']:
                self.assertIn(edge['ref'], refs)
                self.assertLessEqual(set(edge['dependsOn']), refs)
            self.assertIn({'ref': 'archive:release.tar.gz', 'dependsOn': ['file:symeraseme']},
                          document['dependencies'])
            binary.write_bytes(b'rust-fixture-v2')
            changed = sbom.document(archive, binary, INVENTORY, SOURCE_REVISION, ('linux', 'amd64'))
            self.assertNotEqual(rust, next(item for item in changed['components']
                                            if item['bom-ref'] == 'file:symeraseme'))

    def test_real_embedded_audit_provenance_when_native_inputs_are_available(self):
        binary = os.environ.get('SYMERASEME_AUDIT_TEST_BINARY')
        extractor = os.environ.get('SYMERASEME_AUDIT_TEST_EXTRACTOR') or shutil.which('rust-audit-info')
        if not binary or not extractor:
            self.skipTest('set SYMERASEME_AUDIT_TEST_BINARY and provide rust-audit-info to inspect a real auditable binary')
        inventory = sbom.read_inventory(Path(binary), Path(extractor))
        root = next(item for item in inventory['packages'] if item.get('root') is True)
        self.assertEqual(root['name'], 'symeraseme-cli')
        self.assertTrue(inventory['packages'])

    def test_generate_then_independently_read_back_six_rust_only_archives(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist, binaries = root / 'dist', root / 'binaries'
            dist.mkdir()
            binaries.mkdir()
            version = '0.13.0'
            (dist / 'metadata.json').write_text(json.dumps({'version': version}) + '\n')
            archive_paths = []
            for system, arch, extension, rust_name in sbom.TARGETS:
                payload = ('rust-' + system + '-' + arch).encode()
                binary = binaries / ('rust-binary-' + system + '-' + arch)
                binary.write_bytes(payload)
                archive = dist / ('symeraseme_' + version + '_' + system + '_' + arch + '.' + extension)
                members = [(rust_name, payload, 0o755), ('LICENSE', b'license-fixture', 0o644),
                           ('README.md', b'readme-fixture', 0o644)]
                if extension == 'tar.gz':
                    with tarfile.open(archive, 'w:gz') as bundle:
                        for name, data, mode in members:
                            info = tarfile.TarInfo(name)
                            info.size, info.mode = len(data), mode
                            bundle.addfile(info, io.BytesIO(data))
                else:
                    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as bundle:
                        for name, data, mode in members:
                            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                            info.external_attr = (stat.S_IFREG | mode) << 16
                            bundle.writestr(info, data)
                archive_paths.append(archive)
            (dist / 'checksums.txt').write_text(''.join(
                sbom.digest(path) + '  ' + path.name + '\n' for path in sorted(archive_paths)))
            extractor = root / 'fixture-extractor'
            extractor.write_text('#!/usr/bin/env python3\nprint(' + repr(json.dumps(INVENTORY)) + ')\n')
            extractor.chmod(0o755)

            sbom.generate(dist, binaries, extractor, SOURCE_REVISION)
            expected_sidecars = {path.name + suffix for path in archive_paths
                                 for suffix in ('.rust-audit.json', '.cdx.json')}
            (dist / 'orphan.go-build-info.txt').write_text('unexpected')
            with self.assertRaisesRegex(ValueError, 'sidecar set'):
                readback.verify(dist, extractor, SOURCE_REVISION)
            (dist / 'orphan.go-build-info.txt').unlink()
            readback.verify(dist, extractor, SOURCE_REVISION)
            manifest_path = dist / 'sbom-checksums.txt'
            original_manifest = manifest_path.read_bytes()
            lines = original_manifest.decode('ascii').splitlines()
            _, first_name = lines[0].split('  ', 1)
            lines[0] = '0' * 64 + '  ' + first_name
            manifest_path.write_text('\n'.join(lines) + '\n', encoding='ascii')
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                readback.verify(dist, extractor, SOURCE_REVISION)
            manifest_path.write_bytes(original_manifest)
            actual_sidecars = {path.name for path in dist.iterdir()
                               if path.name.endswith(('.rust-audit.json', '.cdx.json'))}
            self.assertEqual(actual_sidecars, expected_sidecars)
            self.assertEqual(len((dist / 'sbom-checksums.txt').read_text().splitlines()), 12)
            for sidecar in dist.glob('*.cdx.json'):
                value = json.loads(sidecar.read_text())
                self.assertEqual(value['metadata']['properties'][0]['value'], SOURCE_REVISION)
                self.assertFalse(any(item.get('bom-ref', '').startswith('file:symeraseme-go')
                                     or item.get('purl', '').startswith('pkg:golang/')
                                     for item in value['components']))
            with self.assertRaisesRegex(ValueError, 'CycloneDX'):
                readback.verify(dist, extractor, 'b' * 40)

    def test_archive_member_mutation_cannot_reuse_unpacked_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, rust = root / 'archive.tar.gz', root / 'rust'
            rust.write_bytes(b'original Rust')
            with tarfile.open(archive, 'w:gz') as bundle:
                for name, raw in [('symeraseme', rust.read_bytes()), ('LICENSE', b'license'),
                                  ('README.md', b'readme')]:
                    item = tarfile.TarInfo(name)
                    item.size = len(raw)
                    bundle.addfile(item, io.BytesIO(raw))
            sbom.verify_archive_binary(archive, rust, 'linux')
            rust.write_bytes(b'modified Rust')
            with self.assertRaisesRegex(ValueError, 'bytes differ'):
                sbom.verify_archive_binary(archive, rust, 'linux')

    def test_archive_rejects_extra_members_and_malformed_audits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, rust = root / 'archive.tar.gz', root / 'rust'
            rust.write_bytes(b'rust')
            with tarfile.open(archive, 'w:gz') as bundle:
                for name, raw in [('symeraseme', b'rust'), ('LICENSE', b'license'),
                                  ('README.md', b'readme'), ('unexpected', b'extra')]:
                    item = tarfile.TarInfo(name)
                    item.size = len(raw)
                    bundle.addfile(item, io.BytesIO(raw))
            with self.assertRaisesRegex(ValueError, 'archive contents'):
                sbom.verify_archive_binary(archive, rust, 'linux')
        for malformed in [None, {'packages': [None]},
                          {'packages': [dict(INVENTORY['packages'][0], dependencies=[{}])]}]:
            with self.subTest(value=malformed), self.assertRaises(ValueError):
                sbom.validate_inventory(malformed)


if __name__ == '__main__':
    unittest.main()
