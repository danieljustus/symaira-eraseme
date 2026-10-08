#!/usr/bin/env python3
"""Read back packed Rust binaries and independently reproduce their SBOMs."""
import argparse
import json
from pathlib import Path
import re
import sys
import tarfile
import tempfile
import zipfile

from rust_artifact_sbom import TARGETS, digest, document, read_inventory, require, validate_source_revision

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tests'))
from verify_release_archives import main as verify_archives

MAX_RELEASE_EXECUTABLE_BYTES = 128 * 1024 * 1024
SIDECAR_SUFFIXES = ('.rust-audit.json', '.cdx.json', '.go-build-info.txt')


def verify(dist, extractor, source_revision):
    validate_source_revision(source_revision)
    require(dist.is_dir() and not dist.is_symlink(), 'release distribution must be a directory')
    metadata_path = dist / 'metadata.json'
    require(metadata_path.is_file() and not metadata_path.is_symlink(),
            'release metadata must be a regular file')
    metadata = json.loads(metadata_path.read_text())
    version = metadata.get('version')
    require(type(version) is str and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9.+-]*', version) is not None,
            'release metadata has an invalid version')
    for path in dist.iterdir():
        if path.name.endswith(('.tar.gz', '.zip')):
            require(path.is_file() and not path.is_symlink(), 'release archive must be a regular file')
    verify_archives(dist)

    expected = set()
    for system, arch, extension, rust_name in TARGETS:
        archive = dist / ('symeraseme_' + version + '_' + system + '_' + arch + '.' + extension)
        expected.update(archive.name + suffix for suffix in ('.rust-audit.json', '.cdx.json'))
        with tempfile.TemporaryDirectory(prefix='release-sbom-readback-') as directory:
            root = Path(directory)
            executable = root / rust_name
            container = zipfile.ZipFile(archive) if system == 'windows' else tarfile.open(archive, 'r:gz')
            with container:
                member = container.getinfo(rust_name) if system == 'windows' else container.getmember(rust_name)
                size = member.file_size if system == 'windows' else member.size
                require(0 < size <= MAX_RELEASE_EXECUTABLE_BYTES,
                        'release executable size exceeds bounds')
                opener = container.open if system == 'windows' else container.extractfile
                with opener(member) as source, executable.open('xb') as output:
                    total = 0
                    for block in iter(lambda: source.read(1024 * 1024), b''):
                        total += len(block)
                        require(total <= size and total <= MAX_RELEASE_EXECUTABLE_BYTES,
                                'packed executable exceeds declared size or extraction bound')
                        output.write(block)
                require(total == size, 'packed executable is truncated')

            inventory = read_inventory(executable, extractor)
            root_package = next(item for item in inventory['packages'] if item.get('root') is True)
            require(root_package['name'] == 'symeraseme-cli' and root_package['version'] == version,
                    'packed Rust root package differs from release identity')
            audit_path = dist / (archive.name + '.rust-audit.json')
            sbom_path = dist / (archive.name + '.cdx.json')
            require(audit_path.is_file() and not audit_path.is_symlink(),
                    'published Rust inventory must be a regular file')
            require(sbom_path.is_file() and not sbom_path.is_symlink(),
                    'published CycloneDX document must be a regular file')
            require(inventory == json.loads(audit_path.read_text()),
                    'published Rust inventory differs from embedded executable data')
            rebuilt = document(archive, executable, inventory, source_revision, (system, arch))
            require(rebuilt == json.loads(sbom_path.read_text()),
                    'published CycloneDX document differs from independently packed Rust inventory')

    sidecars = {path.name for path in dist.iterdir()
                if path.name.endswith(SIDECAR_SUFFIXES)}
    require(sidecars == expected, 'SBOM sidecar set differs from twelve expected Rust-only files')
    manifest_path = dist / 'sbom-checksums.txt'
    require(manifest_path.is_file() and not manifest_path.is_symlink(),
            'SBOM checksum manifest must be a regular file')
    manifest = {}
    for line in manifest_path.read_text(encoding='ascii').splitlines():
        match = re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9_.+-]+)', line)
        require(match is not None, 'invalid SBOM checksum line')
        value, name = match.groups()
        require(name not in manifest, 'duplicate SBOM checksum entry')
        manifest[name] = value
    require(set(manifest) == expected, 'SBOM checksum member set differs from twelve expected sidecars')
    require(all(digest(dist / name) == value for name, value in manifest.items()),
            'SBOM checksum mismatch')
    canonical = ''.join(digest(dist / name) + '  ' + name + '\n' for name in sorted(expected))
    require(manifest_path.read_text(encoding='ascii') == canonical,
            'SBOM checksum manifest is not in canonical order')
    print('PASS: six packed Rust inventories, exact dependency graphs, source identities and twelve sidecar hashes')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist', type=Path, required=True)
    parser.add_argument('--extractor', type=Path, required=True)
    parser.add_argument('--source-revision', required=True)
    args = parser.parse_args()
    verify(args.dist, args.extractor, args.source_revision)
