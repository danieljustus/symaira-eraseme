#!/usr/bin/env python3
"""Generate CycloneDX inventories from the exact auditable release binaries."""
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


def require(condition, message):
    if not condition:
        raise ValueError(message)


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
        require(package.get('kind', 'runtime') in ('runtime', 'build'), 'unknown dependency kind')
        children = package.get('dependencies', [])
        require(type(children) is list, 'invalid dependency list')
        for index in children:
            require(type(index) is int and 0 <= index < len(packages), 'dependency index out of range')
            incoming[index] += 1
        require(len(children) == len(set(children)), 'invalid dependency list')
    require(sum(package.get('root') is True for package in packages) == 1,
            'auditable payload needs exactly one root package')
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
    return packages


def read_inventory(binary, extractor):
    with tempfile.TemporaryDirectory(prefix='rust-artifact-inventory-') as directory:
        out, err = Path(directory) / 'out.json', Path(directory) / 'err.txt'
        with out.open('xb') as stdout, err.open('xb') as stderr:
            result = subprocess.run([str(extractor), str(binary)], stdin=subprocess.DEVNULL,
                                    stdout=stdout, stderr=stderr, check=False, timeout=30)
        require(result.returncode == 0, 'auditable extractor rejected release binary')
        require(out.stat().st_size <= 8 * 1024 * 1024 and err.stat().st_size <= 65536,
                'auditable extractor output exceeded bounds')
        value = json.loads(out.read_bytes())
    validate_inventory(value)
    return value


def go_components(build_info):
    modules, root = [], None
    for line in build_info.splitlines():
        fields = line.split('\t')
        if len(fields) >= 4 and fields[1] in ('mod', 'dep'):
            require(fields[2] and fields[3], 'Go build-info module identity missing')
            component = {'type': 'library', 'bom-ref': 'go:' + str(len(modules)),
                         'name': fields[2], 'version': fields[3]}
            if not fields[3].startswith('('):
                component['purl'] = 'pkg:golang/' + quote(fields[2], safe='/') + '@' + quote(fields[3], safe='')
            if len(fields) > 4 and fields[4]:
                component['properties'] = [{'name': 'go:module_checksum', 'value': fields[4]}]
            if fields[1] == 'mod':
                require(root is None, 'Go build info has duplicate root module')
                root = component['bom-ref']
            modules.append(component)
        elif len(fields) >= 4 and fields[1] == '=>':
            # Preserve the effective replacement's identity as reported by Go.
            require(bool(modules), 'Go replacement without module')
            original = modules[-1]
            original['properties'] = original.get('properties', []) + [
                {'name': 'go:original_module', 'value': original['name'] + '@' + original['version']}]
            original['name'], original['version'] = fields[2], fields[3]
            original.pop('purl', None)
            # A replaced module's old checksum does not identify shipped code.
            original['properties'] = [item for item in original['properties']
                                      if item['name'] != 'go:module_checksum']
            if not fields[3].startswith('('):
                original['purl'] = 'pkg:golang/' + quote(fields[2], safe='/') + '@' + quote(fields[3], safe='')
            if len(fields) > 4 and fields[4]:
                original['properties'].append({'name': 'go:module_checksum', 'value': fields[4]})
    require(root is not None and bool(modules), 'Go fallback has no embedded module inventory')
    return modules, root


def canonical_go_info(raw):
    lines = raw.splitlines()
    require(bool(lines) and ': go' in lines[0], 'Go build-info header missing')
    return 'symeraseme-go: ' + lines[0].rsplit(': ', 1)[1] + '\n' + '\n'.join(lines[1:]) + '\n'


def verify_archive_binaries(archive, binary, go_binary, system):
    names = ['symeraseme', 'symeraseme-go']
    if system == 'windows':
        names = [name + '.exe' for name in names]
    if system == 'windows':
        container = zipfile.ZipFile(archive)
        entries = container.infolist()
        members = lambda name: [item for item in entries if item.filename == name]
        opener = container.open
    else:
        container = tarfile.open(archive, mode='r:gz')
        entries = container.getmembers()
        members = lambda name: [item for item in entries if item.name == name and item.isfile()]
        opener = container.extractfile
    with container:
        for name, path in zip(names, (binary, go_binary)):
            selected = members(name)
            require(len(selected) == 1, 'archive executable member missing or duplicated')
            size = selected[0].file_size if system == 'windows' else selected[0].size
            require(size == path.stat().st_size, 'archive executable size differs from SBOM input')
            actual, total = hashlib.sha256(), 0
            with opener(selected[0]) as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    total += len(block)
                    require(total <= size, 'archive executable exceeds declared size')
                    actual.update(block)
            require(total == size and actual.hexdigest() == digest(path),
                    'archive executable bytes differ from SBOM input')


def document(archive, binary, go_binary, inventory, source_revision, target, go_build_info):
    packages = validate_inventory(inventory)
    archive_ref, rust_ref, go_ref = 'archive:' + archive.name, 'file:symeraseme', 'file:symeraseme-go'
    if target[0] == 'windows':
        rust_ref += '.exe'
        go_ref += '.exe'
    def file_component(reference, name, path, kind):
        return {'type': kind, 'bom-ref': reference, 'name': name,
                'hashes': [{'alg': 'SHA-256', 'content': digest(path)}]}
    rust = file_component(rust_ref, rust_ref.removeprefix('file:'), binary, 'application')
    go = file_component(go_ref, go_ref.removeprefix('file:'), go_binary, 'application')
    go['properties'] = [{'name': 'symeraseme:role', 'value': 'explicit Go rollback fallback'},
                        {'name': 'symeraseme:inventory_origin', 'value': 'embedded native Go build info'}]
    components, dependencies = [rust, go], []
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
    modules, go_root = go_components(go_build_info)
    components.extend(modules)
    dependencies.append({'ref': go_root, 'dependsOn': [item['bom-ref'] for item in modules if item['bom-ref'] != go_root]})
    dependencies.extend([{'ref': archive_ref, 'dependsOn': [rust_ref, go_ref]},
                         {'ref': rust_ref, 'dependsOn': ['cargo:' + str(root_index)]},
                         {'ref': go_ref, 'dependsOn': [go_root]}])
    return {'bomFormat': 'CycloneDX', 'specVersion': '1.6', 'version': 1,
            'metadata': {'component': file_component(archive_ref, archive.name, archive, 'file'),
                         'properties': [{'name': 'symeraseme:source_revision', 'value': source_revision},
                                        {'name': 'symeraseme:native_target', 'value': '-'.join(target)},
                                        {'name': 'symeraseme:inventory_origin', 'value': 'embedded cargo-auditable .dep-v0 section'}]},
            'components': components, 'dependencies': dependencies}


def generate(dist, binaries, extractor, source_revision):
    require(re.fullmatch('[0-9a-f]{40}', source_revision) is not None, 'full source revision required')
    version = json.loads((dist / 'metadata.json').read_text())['version']
    generated = []
    for system, arch, extension, _ in TARGETS:
        archive = dist / ('symeraseme_' + version + '_' + system + '_' + arch + '.' + extension)
        binary = binaries / ('rust-binary-' + system + '-' + arch)
        go_binary = binaries / ('go-binary-' + system + '-' + arch)
        verify_archive_binaries(archive, binary, go_binary, system)
        inventory = read_inventory(binary, extractor)
        root_package = next(item for item in inventory['packages'] if item.get('root') is True)
        require(root_package['name'] == 'symeraseme-cli' and root_package['version'] == version,
                'embedded Rust root package differs from release identity')
        info = binaries / ('go-build-info-' + system + '-' + arch + '.txt')
        raw_info = canonical_go_info(info.read_text())
        settings = {line.strip() for line in raw_info.splitlines()}
        require('build\tvcs.revision=' + source_revision in settings and 'build\tvcs.modified=false' in settings,
                'Go fallback build identity does not match the native source')
        require('build\tGOOS=' + system in settings and 'build\tGOARCH=' + arch in settings,
                'Go fallback embedded target differs from archive target')
        audit = dist / (archive.name + '.rust-audit.json')
        audit.write_text(json.dumps(inventory, indent=2) + '\n')
        go_info = dist / (archive.name + '.go-build-info.txt')
        go_info.write_text(raw_info)
        sbom = dist / (archive.name + '.cdx.json')
        sbom.write_text(json.dumps(document(archive, binary, go_binary, inventory, source_revision,
                                           (system, arch), raw_info), indent=2) + '\n')
        generated.extend([audit, go_info, sbom])
    # Retain the archive verifier's six-entry checksums contract. Additional
    # release metadata gets its own exact-name manifest and later signatures.
    (dist / 'sbom-checksums.txt').write_text(''.join(digest(path) + '  ' + path.name + '\n'
                                                 for path in sorted(generated)))
    print('Generated six artifact-bound Rust CycloneDX inventories with native Go fallback build info')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--extractor', type=Path, required=True)
    parser.add_argument('--source-revision', required=True)
    args = parser.parse_args()
    generate(args.dist, args.binaries, args.extractor, args.source_revision)
