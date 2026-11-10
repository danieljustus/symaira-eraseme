"""Fetch a checksum- and release-digest-bound native historical Go archive.

Build/download preparation runs outside the CLI confinement boundary. Extraction
copies only the verified regular top-level executable; no archive paths are used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import tarfile
import urllib.request
import urllib.error

import backup_restore_rehearsal as rehearsal

REPOSITORY = 'danieljustus/symaira-eraseme'
RELEASE = 'v0.12.1'
RELEASE_METADATA_URL = 'https://api.github.com/repos/' + REPOSITORY + '/releases/tags/' + RELEASE


class NoCredentialRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        # Metadata has one fixed API URL. Never forward its CI token to a
        # redirect target; archive/checksum downloads remain anonymous.
        raise urllib.error.HTTPError(request.full_url, code,
                                     'authenticated release metadata redirect refused', headers, response)


def native_architecture():
    return {'x86_64': 'amd64', 'arm64': 'arm64', 'aarch64': 'arm64'}.get(platform.machine())


def fetch(url, maximum):
    rehearsal.require(url.startswith(('https://api.github.com/', 'https://github.com/')),
                      'release URL must belong to GitHub')
    request = urllib.request.Request(url, headers={'User-Agent': 'EraseMe-native-restore-proof',
                                                 'Accept': 'application/vnd.github+json'})
    token = os.environ.get('ERASEME_RELEASE_METADATA_TOKEN') if url == RELEASE_METADATA_URL else None
    if token:
        request.add_header('Authorization', 'Bearer ' + token)
        open_request = urllib.request.build_opener(NoCredentialRedirect()).open
    else:
        open_request = urllib.request.urlopen
    with open_request(request, timeout=30) as response:
        raw = response.read(maximum + 1)
    rehearsal.require(len(raw) <= maximum, 'release response exceeded its byte bound')
    return raw


def prepare(destination):
    destination = Path(destination)
    rehearsal.require(not destination.is_symlink(), 'release input directory must not be a symlink')
    destination.mkdir(parents=True, exist_ok=False)
    system = {'Linux': 'linux', 'Darwin': 'darwin'}.get(platform.system())
    arch = native_architecture()
    rehearsal.require(system and arch, 'unsupported native release platform')
    name = 'symeraseme_0.12.1_' + system + '_' + arch + '.tar.gz'
    release = json.loads(fetch(RELEASE_METADATA_URL, 1024 * 1024))
    rehearsal.require(release['tag_name'] == RELEASE and not release['draft'] and not release['prerelease'],
                      'historical release identity changed')
    def asset(asset_name):
        matches = [item for item in release['assets'] if item['name'] == asset_name]
        rehearsal.require(len(matches) == 1, 'expected exactly one release asset: ' + asset_name)
        item = matches[0]
        rehearsal.require(0 < item['size'] <= 32 * 1024 * 1024, 'unexpected release asset size')
        raw = fetch(item['browser_download_url'], item['size'])
        digest = hashlib.sha256(raw).hexdigest()
        rehearsal.require(len(raw) == item['size'] and item['digest'] == 'sha256:' + digest,
                          'release asset bytes do not match published size/digest')
        return raw, item
    checksums, checksums_asset = asset('checksums.txt')
    rows = [line.split() for line in checksums.decode('ascii').splitlines() if line.strip()]
    matches = [row[0] for row in rows if len(row) == 2 and row[1].lstrip('*') == name]
    rehearsal.require(len(matches) == 1, 'expected exactly one archive checksum')
    raw, archive_asset = asset(name)
    rehearsal.require(hashlib.sha256(raw).hexdigest() == matches[0], 'archive checksum differs')
    archive = destination / name
    archive.write_bytes(raw)
    member = 'symeraseme'
    with tarfile.open(archive) as container:
        entries = [entry for entry in container.getmembers() if entry.name == member]
        rehearsal.require(len(entries) == 1 and entries[0].isfile(), 'archive executable must be regular')
        rehearsal.require(0 < entries[0].size <= 32 * 1024 * 1024, 'executable size exceeded bound')
        with container.extractfile(entries[0]) as stream:
            binary = stream.read(32 * 1024 * 1024 + 1)
    executable = destination / 'retained-go'
    executable.write_bytes(binary)
    executable.chmod(0o700)
    verified = rehearsal.verify_go_archive(archive, executable)
    evidence = {'release': RELEASE, 'repository': REPOSITORY, 'platform': platform.system(),
                'machine': platform.machine(), 'native_architecture': arch, 'archive': str(archive.resolve()),
                'executable': str(executable.resolve()), 'verified_archive': verified,
                'release_asset': archive_asset, 'checksums_asset': checksums_asset}
    (destination / 'release-inputs.json').write_text(json.dumps(evidence, indent=2) + '\n')
    (destination / 'checksums.txt').write_bytes(checksums)
    return evidence


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, required=True)
    result = prepare(parser.parse_args().output_dir)
    print(json.dumps({'archive': result['archive'], 'executable': result['executable']}))
