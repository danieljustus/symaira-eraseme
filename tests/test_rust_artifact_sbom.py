"""Artifact dependency identity and malformed-graph controls."""
import copy
import hashlib
import io
from pathlib import Path
import sys
import tempfile
import tarfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import rust_artifact_sbom as sbom

INVENTORY = {'packages': [
    {'name': 'symeraseme-cli', 'version': '0.13.0', 'source': 'local', 'root': True, 'dependencies': [1]},
    {'name': 'dependency', 'version': '1.2.3', 'source': 'registry'},
]}
GO_INFO = 'go-bin: go1.26.6\n\tmod\tgithub.com/danieljustus/symaira-eraseme\t(devel)\t\n\tdep\texample.org/dependency\tv1.2.3\th1:example\n'


class ArtifactSbomControls(unittest.TestCase):
    def test_embedded_graph_rejects_cycle_multiple_roots_and_invalid_indexes(self):
        cycle = copy.deepcopy(INVENTORY)
        cycle['packages'][1]['dependencies'] = [0]
        roots = copy.deepcopy(INVENTORY)
        roots['packages'][1]['root'] = True
        index = copy.deepcopy(INVENTORY)
        index['packages'][0]['dependencies'] = [99]
        with self.assertRaisesRegex(ValueError, 'cycle'):
            sbom.validate_inventory(cycle)
        with self.assertRaisesRegex(ValueError, 'exactly one root'):
            sbom.validate_inventory(roots)
        with self.assertRaisesRegex(ValueError, 'out of range'):
            sbom.validate_inventory(index)
        boolean = copy.deepcopy(INVENTORY)
        boolean['packages'][0]['dependencies'] = [True]
        with self.assertRaisesRegex(ValueError, 'out of range'):
            sbom.validate_inventory(boolean)

    def test_archive_and_both_executable_hashes_bind_dependency_components(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, binary, fallback = root / 'release.tar.gz', root / 'rust', root / 'go'
            for path, raw in [(archive, b'archive'), (binary, b'rust'), (fallback, b'go')]:
                path.write_bytes(raw)
            document = sbom.document(archive, binary, fallback, INVENTORY, 'a'*40,
                                     ('linux', 'amd64'), GO_INFO)
            expected = hashlib.sha256(b'archive').hexdigest()
            self.assertEqual(document['metadata']['component']['hashes'][0]['content'], expected)
            rust = next(x for x in document['components'] if x['bom-ref'] == 'file:symeraseme')
            self.assertEqual(rust['hashes'][0]['content'], hashlib.sha256(b'rust').hexdigest())
            self.assertTrue(any(x.get('purl') == 'pkg:cargo/dependency@1.2.3' for x in document['components']))
            self.assertTrue(any(x.get('purl') == 'pkg:golang/example.org/dependency@v1.2.3' for x in document['components']))
            refs = {x['bom-ref'] for x in document['components']} | {document['metadata']['component']['bom-ref']}
            for edge in document['dependencies']:
                self.assertIn(edge['ref'], refs)
                self.assertLessEqual(set(edge['dependsOn']), refs)
            binary.write_bytes(b'different Rust binary')
            changed = sbom.document(archive, binary, fallback, INVENTORY, 'a'*40,
                                   ('linux', 'amd64'), GO_INFO)
            self.assertNotEqual(rust, next(x for x in changed['components'] if x['bom-ref'] == 'file:symeraseme'))

    def test_missing_go_root_is_not_an_artifact_inventory(self):
        with self.assertRaisesRegex(ValueError, 'no embedded module inventory'):
            sbom.go_components('go-bin: go1.26.6\n')

    def test_replacement_records_shipped_module_instead_of_original_identity(self):
        info = GO_INFO + '\t=>\texample.org/patched\tv1.2.4\th1:patched\n'
        modules, _ = sbom.go_components(info)
        effective = modules[-1]
        self.assertEqual(effective['purl'], 'pkg:golang/example.org/patched@v1.2.4')
        self.assertIn({'name': 'go:original_module', 'value': 'example.org/dependency@v1.2.3'}, effective['properties'])
        self.assertNotIn({'name': 'go:module_checksum', 'value': 'h1:example'}, effective['properties'])

    def test_archive_member_mutation_cannot_reuse_an_unpacked_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, rust, go = root / 'archive.tar.gz', root / 'rust', root / 'go'
            rust.write_bytes(b'original Rust'); go.write_bytes(b'original Go')
            with tarfile.open(archive, 'w:gz') as bundle:
                for name, raw in [('symeraseme', rust.read_bytes()), ('symeraseme-go', go.read_bytes())]:
                    item = tarfile.TarInfo(name); item.size = len(raw)
                    bundle.addfile(item, io.BytesIO(raw))
            sbom.verify_archive_binaries(archive, rust, go, 'linux')
            rust.write_bytes(b'modified Rust')
            with self.assertRaisesRegex(ValueError, 'bytes differ'):
                sbom.verify_archive_binaries(archive, rust, go, 'linux')

    def test_malformed_embedded_objects_fail_closed(self):
        for malformed in [None, {'packages': [None]}, {'packages': [dict(INVENTORY['packages'][0], dependencies=[{}])]}]:
            with self.subTest(value=malformed), self.assertRaises(ValueError):
                sbom.validate_inventory(malformed)


if __name__ == '__main__':
    unittest.main()
