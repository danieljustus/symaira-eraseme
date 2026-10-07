#!/usr/bin/env python3
"""Generate CycloneDX inventories from the exact auditable Rust release binaries."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import tarfile
import zipfile
from urllib.parse import quote

from stage_rust_release_archives import TARGETS

MAX_INVENTORY_BYTES = 8 * 1024 * 1024
MAX_EXTRACTOR_STDERR_BYTES = 65536


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_source_revision(source_revision):
    require(type(source_revision) is str and re.fullmatch(r'[0-9a-f]{40}', source_revision) is not None,
            'full source revision required')


def digest(path):
    require(path.is_file() and not path.is_symlink(), 'artifact must be a regular file')
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def validate_inventory(value):
    require(type(value) is dict and type(value.get('packages')) is list,
            'auditable payload must contain packages')
    packages = value['packages']
    require(0 < len(packages) <= 10000, 'invalid auditable package count')
    incoming = [0] * len(packages)
    for package in packages:
        require(type(package) is dict and all(type(package.get(key)) is str and package[key]
                                              for key in ('name', 'version', 'source')),
                'auditable package identity is incomplete')
        if 'root' in package:
            require(type(package['root']) is bool, 'invalid root package marker')
        require(package.get('kind', 'runtime') in ('runtime', 'build'), 'unknown dependency kind')
        children = package.get('dependencies', [])
        require(type(children) is list, 'invalid dependency list')
        for index in children:
            require(type(index) is int and 0 <= index < len(packages), 'dependency index out of range')
            incoming[index] += 1
        require(len(children) == len(set(children)), 'invalid dependency list')
    roots = [index for index, package in enumerate(packages) if package.get('root') is True]
    require(len(roots) == 1, 'auditable payload needs exactly one root package')
    root_index = roots[0]
    root_incoming = incoming[root_index]

    ready = [index for index, count in enumerate(incoming) if count == 0]
    visited = 0
    while ready:
        index = ready.pop()
        visited += 1
        for child in packages[index].get('dependencies', []):
            incoming[child] -= 1
            if incoming[child] == 0:
                ready.append(child)
    require(visited == len(packages), 'auditable dependency graph contains a cycle')
    require(root_incoming == 0, 'root package cannot be a dependency')

    reachable, pending = set(), [root_index]
    while pending:
        index = pending.pop()
        if index not in reachable:
            reachable.add(index)
            pending.extend(packages[index].get('dependencies', []))
    require(len(reachable) == len(packages), 'auditable dependency graph has unreachable packages')
    return packages


def read_inventory(binary, extractor):
    with tempfile.TemporaryDirectory(prefix='rust-artifact-inventory-') as directory:
        out, err = Path(directory) / 'out.json', Path(directory) / 'err.txt'
        with out.open('xb') as stdout, err.open('xb') as stderr:
            result = subprocess.run([str(extractor), str(binary)], stdin=subprocess.DEVNULL,
                                    stdout=stdout, stderr=stderr, check=False, timeout=30)
        require(result.returncode == 0, 'auditable extractor rejected release binary')
        require(out.stat().st_size <= MAX_INVENTORY_BYTES
                and err.stat().st_size <= MAX_EXTRACTOR_STDERR_BYTES,
                'auditable extractor output exceeded bounds')
        value = json.loads(out.read_bytes())
    validate_inventory(value)
    return value


def verify_archive_binary(archive, binary, system):
    executable = 'symeraseme.exe' if system == 'windows' else 'symeraseme'
    expected = {executable, 'LICENSE', 'README.md'}
    require(archive.is_file() and not archive.is_symlink(), 'release archive must be a regular file')
    require(binary.is_file() and not binary.is_symlink(), 'Rust executable must be a regular file')
    if system == 'windows':
        container = zipfile.ZipFile(archive)
        entries = container.infolist()
        names = [item.filename for item in entries]
        members = lambda name: [item for item in entries if item.filename == name]
        opener = container.open
        regular = lambda item: (not item.is_dir()
                                and ((item.external_attr >> 16) & 0o170000 in (0, 0o100000)))
        size_of = lambda item: item.file_size
    else:
        container = tarfile.open(archive, mode='r:gz')
        entries = container.getmembers()
        names = [item.name for item in entries]
        members = lambda name: [item for item in entries if item.name == name]
        opener = container.extractfile
        regular = lambda item: item.isfile()
        size_of = lambda item: item.size
    with container:
        require(len(names) == len(set(names)) and set(names) == expected,
                'archive contents differ from Rust-only release contract')
        require(all(regular(item) for item in entries), 'archive members must be regular files')
        selected = members(executable)
        require(len(selected) == 1, 'archive executable member missing or duplicated')
        size = size_of(selected[0])
        require(size == binary.stat().st_size, 'archive executable size differs from SBOM input')
        actual, total = hashlib.sha256(), 0
        with opener(selected[0]) as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                total += len(block)
                require(total <= size, 'archive executable exceeds declared size')
                actual.update(block)
        require(total == size and actual.hexdigest() == digest(binary),
                'archive executable bytes differ from SBOM input')


def document(archive, binary, inventory, source_revision, target):
    validate_source_revision(source_revision)
    packages = validate_inventory(inventory)
    system, _ = target
    archive_ref = 'archive:' + archive.name
    executable = 'symeraseme.exe' if system == 'windows' else 'symeraseme'
    rust_ref = 'file:' + executable

    def file_component(reference, name, path, kind):
        return {'type': kind, 'bom-ref': reference, 'name': name,
                'hashes': [{'alg': 'SHA-256', 'content': digest(path)}]}

    rust = file_component(rust_ref, executable, binary, 'application')
    components, dependencies = [rust], []
    root_index = next(index for index, package in enumerate(packages) if package.get('root') is True)
    for index, package in enumerate(packages):
        component = {'type': 'library', 'bom-ref': 'cargo:' + str(index),
                     'name': package['name'], 'version': package['version'],
                     'scope': 'excluded' if package.get('kind') == 'build' else 'required',
                     'properties': [{'name': 'cargo:source', 'value': package['source']},
                                    {'name': 'cargo:kind', 'value': package.get('kind', 'runtime')}]}
        if package['source'] in ('registry', 'crates.io'):
            component['purl'] = 'pkg:cargo/' + quote(package['name'], safe='') + '@' + quote(package['version'], safe='')
        components.append(component)
        dependencies.append({'ref': component['bom-ref'],
                             'dependsOn': ['cargo:' + str(child) for child in package.get('dependencies', [])]})

    dependencies.extend([{'ref': archive_ref, 'dependsOn': [rust_ref]},
                         {'ref': rust_ref, 'dependsOn': ['cargo:' + str(root_index)]}])
    return {'bomFormat': 'CycloneDX', 'specVersion': '1.6', 'version': 1,
            'metadata': {'component': file_component(archive_ref, archive.name, archive, 'file'),
                         'properties': [{'name': 'symeraseme:source_revision', 'value': source_revision},
                                        {'name': 'symeraseme:native_target', 'value': '-'.join(target)},
                                        {'name': 'symeraseme:inventory_origin',
                                         'value': 'embedded cargo-auditable .dep-v0 section'}]},
            'components': components, 'dependencies': dependencies}


def generate(dist, binaries, extractor, source_revision):
    validate_source_revision(source_revision)
    metadata_path = dist / 'metadata.json'
    require(metadata_path.is_file() and not metadata_path.is_symlink(),
            'release metadata must be a regular file')
    version = json.loads(metadata_path.read_text())['version']
    require(type(version) is str and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9.+-]*', version) is not None,
            'release metadata has an invalid version')

    outputs = []
    for system, arch, extension, _ in TARGETS:
        archive = dist / ('symeraseme_' + version + '_' + system + '_' + arch + '.' + extension)
        binary = binaries / ('rust-binary-' + system + '-' + arch)
        verify_archive_binary(archive, binary, system)
        inventory = read_inventory(binary, extractor)
        root_package = next(item for item in inventory['packages'] if item.get('root') is True)
        require(root_package['name'] == 'symeraseme-cli' and root_package['version'] == version,
                'embedded Rust root package differs from release identity')
        sbom = document(archive, binary, inventory, source_revision, (system, arch))
        outputs.extend([(dist / (archive.name + '.rust-audit.json'), json.dumps(inventory, indent=2) + '\n'),
                        (dist / (archive.name + '.cdx.json'), json.dumps(sbom, indent=2) + '\n')])

    expected_sidecars = {path.name for path, _ in outputs}
    existing_sidecars = {path.name for path in dist.iterdir()
                         if path.name.endswith(('.rust-audit.json', '.cdx.json', '.go-build-info.txt'))}
    require(existing_sidecars <= expected_sidecars,
            'unexpected SBOM sidecar in Rust-only distribution')
    checksums = dist / 'sbom-checksums.txt'
    for path, _ in outputs:
        require(not path.is_symlink() and (not path.exists() or path.is_file()),
                'SBOM sidecars must be regular files')
    require(not checksums.is_symlink() and (not checksums.exists() or checksums.is_file()),
            'SBOM checksum manifest must be a regular file')
    for path, content in outputs:
        path.write_text(content, encoding='utf-8')
    checksums.write_text(''.join(digest(path) + '  ' + path.name + '\n'
                                 for path, _ in sorted(outputs, key=lambda item: item[0].name)),
                         encoding='ascii')
    print('Generated six artifact-bound Rust CycloneDX inventories')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--extractor', type=Path, required=True)
    parser.add_argument('--source-revision', required=True)
    args = parser.parse_args()
    generate(args.dist, args.binaries, args.extractor, args.source_revision)
