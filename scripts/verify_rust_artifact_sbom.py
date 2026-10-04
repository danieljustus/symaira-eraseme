#!/usr/bin/env python3
"""Read back packed binaries and independently reproduce their published SBOMs."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import zipfile

from rust_artifact_sbom import TARGETS, canonical_go_info, digest, document, read_inventory, require

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tests'))
from verify_release_archives import main as verify_archives


def verify(dist, extractor, go_tool, source_revision):
    require(re.fullmatch('[0-9a-f]{40}', source_revision) is not None, 'full source revision required')
    verify_archives(dist, require_go_fallback=True)
    version = json.loads((dist / 'metadata.json').read_text())['version']
    expected = set()
    for system, arch, extension, rust_name in TARGETS:
        archive = dist / ('symeraseme_' + version + '_' + system + '_' + arch + '.' + extension)
        expected.update(archive.name + suffix for suffix in ('.rust-audit.json', '.go-build-info.txt', '.cdx.json'))
        fallback = 'symeraseme-go.exe' if system == 'windows' else 'symeraseme-go'
        with tempfile.TemporaryDirectory(prefix='release-sbom-readback-') as directory:
            root = Path(directory)
            container = zipfile.ZipFile(archive) if system == 'windows' else tarfile.open(archive, 'r:gz')
            with container:
                for name in (rust_name, fallback):
                    member = container.getinfo(name) if system == 'windows' else container.getmember(name)
                    size = member.file_size if system == 'windows' else member.size
                    require(0 < size <= 128 * 1024 * 1024, 'release executable size exceeds bounds')
                    opener = container.open if system == 'windows' else container.extractfile
                    with opener(member) as source, (root / name).open('xb') as output:
                        total = 0
                        for block in iter(lambda: source.read(1024 * 1024), b''):
                            total += len(block)
                            require(total <= size, 'packed executable exceeds declared size')
                            output.write(block)
                    require(total == size, 'packed executable is truncated')
            inventory = read_inventory(root / rust_name, extractor)
            root_package = next(item for item in inventory['packages'] if item.get('root') is True)
            require(root_package['name'] == 'symeraseme-cli' and root_package['version'] == version,
                    'packed Rust root package differs from release identity')
            require(inventory == json.loads((dist / (archive.name + '.rust-audit.json')).read_text()),
                    'published Rust inventory differs from embedded executable data')
            out, err = root / 'go-info.txt', root / 'go-info.stderr'
            with out.open('xb') as stdout, err.open('xb') as stderr:
                result = subprocess.run([str(go_tool), 'version', '-m', str(root / fallback)],
                                        stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, timeout=30)
            require(result.returncode == 0 and out.stat().st_size <= 1024 * 1024
                    and err.stat().st_size <= 65536, 'Go embedded build-info extraction failed')
            info = canonical_go_info(out.read_text())
            require(info == (dist / (archive.name + '.go-build-info.txt')).read_text(),
                    'published Go inventory differs from packed executable data')
            settings = {line.strip() for line in info.splitlines()}
            require('build\tvcs.revision=' + source_revision in settings and 'build\tvcs.modified=false' in settings,
                    'packed Go fallback source identity differs from published candidate')
            require('build\tGOOS=' + system in settings and 'build\tGOARCH=' + arch in settings,
                    'packed Go fallback target differs from archive')
            rebuilt = document(archive, root / rust_name, root / fallback, inventory,
                               source_revision, (system, arch), info)
            require(rebuilt == json.loads((dist / (archive.name + '.cdx.json')).read_text()),
                    'published CycloneDX document differs from independently packed binary inventories')
    manifest = {}
    for line in (dist / 'sbom-checksums.txt').read_text().splitlines():
        match = re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9_.+-]+)', line)
        require(match is not None, 'invalid SBOM checksum line')
        value, name = match.groups()
        require(name not in manifest, 'duplicate SBOM checksum entry')
        manifest[name] = value
    require(set(manifest) == expected, 'SBOM checksum member set differs from eighteen expected sidecars')
    require(all(digest(dist / name) == value for name, value in manifest.items()), 'SBOM checksum mismatch')
    print('PASS: six packed native inventories, exact SBOM graphs, source identities and eighteen sidecar hashes')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist', type=Path, required=True)
    parser.add_argument('--extractor', type=Path, required=True)
    parser.add_argument('--go-tool', type=Path, required=True)
    parser.add_argument('--source-revision', required=True)
    args = parser.parse_args()
    verify(args.dist, args.extractor, args.go_tool, args.source_revision)
