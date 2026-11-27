#!/usr/bin/env python3
"""Verify downloaded native capture artifacts and prepare the frozen-reader import.

Inputs are the run ID, the directory holding the three downloaded artifact ZIPs,
a coordinator-produced JSON file with the GitHub API SHA-256 digests, the product
candidate SHA and the predecessor refresh id. The tool validates artifact
digests, native targets and provenance, checks the complete 48-record inventory
against the predecessor, accepts only the documented volatile categories via
typed JSON-path validators, and then copies the verified record bytes into a
new fixture directory and repoints the seven frozen readers. Any other change
produces a nonzero exit and a machine-readable report naming the offending
paths. The import is always a proposal for human review; no assertion is
relaxed and no gate is executed by this tool.
"""
import argparse
import base64
from collections import Counter
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import zipfile

RETIREMENT = '22615ae67325c581fbe60cb0f2d3d2b2139cf900'
GO_VERSION = 'go1.26.6'
SUPPORTED_TARGETS = ('darwin/arm64', 'linux/amd64', 'linux/arm64')
TARGET_NAMES = {
    'macos-arm64': 'darwin/arm64',
    'linux-amd64': 'linux/amd64',
    'linux-arm64': 'linux/arm64',
}
FIXTURE_ROOT = 'tests/fixtures/go-frozen'
RECORD_PREFIX = 'frozen-go-'
HEX40 = re.compile(r'^[0-9a-f]{40}$')
HEX8 = re.compile(r'^[0-9a-f]{8}$')
HEX64 = re.compile(r'^[0-9a-f]{64}$')
TOKEN64 = re.compile(r'\b[0-9a-f]{64}\b')
READERS = (
    'crates/symeraseme-cli/tests/support/frozen_agent_error.rs',
    'crates/symeraseme-cli/tests/support/frozen_http_aux.rs',
    'crates/symeraseme-cli/tests/support/frozen_http_headers.rs',
    'crates/symeraseme-cli/tests/support/frozen_http_wire.rs',
    'crates/symeraseme-cli/tests/support/frozen_native_triage.rs',
    'crates/symeraseme-cli/tests/support/frozen_plan_process.rs',
    'crates/symeraseme-core/tests/support/frozen_smtp.rs',
)
CATEGORIES = (
    'source revision (including Go pseudo-version and vcs.revision/vcs.time)',
    'temporary build paths',
    'Cargo.lock digest and bytes',
    'HTTP Date headers and derived raw-response bytes',
    'ephemeral SMTP ports and derived stdin bytes',
    'elapsed times',
    'Go map order in one provider list',
)


class VerificationError(RuntimeError):
    """A required capture invariant failed; report the offending paths."""

    def __init__(self, message, rejections=None, report=None):
        super().__init__(message)
        self.rejections = rejections or []
        self.report = report


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git(repo, *args):
    result = subprocess.run(['git', '-C', str(repo), *args], stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE)
    if result.returncode:
        raise VerificationError('git ' + ' '.join(args) + ' failed: '
                                + result.stderr.decode('utf-8', 'replace').strip())
    return result.stdout


def parts(path):
    return [item for item in path.split('/') if item]


def normalize(path):
    return '/' + '/'.join('<i>' if item.isdigit() else item for item in parts(path))


def leaf_at(root, path):
    node = root
    for item in parts(path):
        node = node[int(item)] if isinstance(node, list) else node[item]
    return node


def compare(old, new, path=''):
    """Yield (concrete JSON path, old leaf, new leaf) for every value change.

    Key sets and list lengths are compared explicitly, so additions and
    deletions can never be hidden by a leaf-level whitelist.
    """
    if isinstance(old, dict) and isinstance(new, dict):
        if set(old) != set(new):
            for key in sorted(set(old) ^ set(new)):
                yield path + '/' + key, old.get(key, '<missing>'), new.get(key, '<missing>')
        for key in old.keys() & new.keys():
            yield from compare(old[key], new[key], path + '/' + key)
    elif isinstance(old, list) and isinstance(new, list):
        if len(old) != len(new):
            yield path + '/<length>', len(old), len(new)
        for index in range(min(len(old), len(new))):
            yield from compare(old[index], new[index], path + '/' + str(index))
    elif type(old) is not type(new) or old != new:
        yield path, old, new


def sibling_path(path, field):
    items = parts(path)
    if items[-1] == 'sha256' and len(items) >= 2:
        return '/'.join(items[:-2] + [items[-2] + '_base64'])
    if items[-1].endswith('_base64'):
        return '/'.join(items[:-1] + [items[-1][:-len('_base64')], 'sha256'])
    raise VerificationError('expected a digest-linked leaf: ' + path)


def strict_base64(value, path):
    if not isinstance(value, str):
        raise VerificationError(path + ': base64 leaf is not a string')
    try:
        return base64.b64decode(value.encode('ascii'), validate=True)
    except (ValueError, UnicodeEncodeError) as error:
        raise VerificationError(path + ': invalid base64: ' + str(error))


def parse_http_date(value):
    try:
        parsed = datetime.strptime(value, '%a, %d %b %Y %H:%M:%S GMT')
        return parsed.strftime('%a, %d %b %Y %H:%M:%S GMT') == value
    except ValueError:
        return False


def parse_utc_time(value):
    try:
        parsed = datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ')
        return parsed.strftime('%Y-%m-%dT%H:%M:%SZ') == value
    except ValueError:
        return False


def parse_pseudo_version(value):
    """Validate the Go pseudo-version shape vMAJOR.MINOR.PATCH-0.TIMESTAMP-REVISION."""
    pieces = value.split('-')
    if len(pieces) < 3:
        return None
    version = '-'.join(pieces[:-2])
    timestamp = pieces[-2]
    revision = pieces[-1]
    if '.' in timestamp:
        prefix, timestamp = timestamp.rsplit('.', 1)
        if not prefix.isdigit():
            return None
    numbers = version.lstrip('v').split('.')
    if not version.startswith('v') or len(numbers) != 3 or not all(n.isdigit() for n in numbers):
        return None
    if len(timestamp) != 14 or not timestamp.isdigit():
        return None
    if not HEX40.match(revision) and not (len(revision) == 12 and re.fullmatch(r'[0-9a-f]+', revision)):
        return None
    return revision


class CheckContext:
    def __init__(self, snapshot, predecessor_snapshot, candidate, lock_bytes, lock_digest):
        self.snapshot = snapshot
        self.predecessor_snapshot = predecessor_snapshot
        self.candidate = candidate
        self.lock_bytes = lock_bytes
        self.lock_digest = lock_digest


def valid_revision(old, new, path, context):
    if not (HEX40.match(old) and HEX40.match(new)):
        return 'revision is not a 40-hex SHA'
    if old != context.predecessor_snapshot:
        return 'predecessor revision does not match the predecessor snapshot'
    if new != context.snapshot:
        return 'new revision does not match capture-identity.capture_revision'
    return None


def valid_lock_bytes(old, new, path, context):
    if not isinstance(old, int) or not isinstance(new, int) or isinstance(new, bool):
        return 'Cargo.lock byte count is not an integer'
    if new != context.lock_bytes:
        return 'Cargo.lock byte count does not match the candidate commit'
    return None


def valid_lock_digest(old, new, path, context):
    if not HEX64.match(new):
        return 'Cargo.lock SHA-256 is not 64 lowercase hex characters'
    if new != context.lock_digest:
        return 'Cargo.lock SHA-256 does not match the candidate commit'
    return None


def valid_build_info(old, new, path, context):
    if not isinstance(old, str) or not isinstance(new, str):
        return 'embedded build info is not a string'
    before, after = old.splitlines(), new.splitlines()
    if len(before) != len(after) or len(before) < 2:
        return 'embedded build info line structure changed'
    first_old, first_new = before[0], after[0]
    if ': ' not in first_old or ': ' not in first_new:
        return 'build executable header is malformed'
    exe_old, go_old = first_old.rsplit(': ', 1)
    exe_new, go_new = first_new.rsplit(': ', 1)
    if go_old != go_new or go_new != 'go1.26.6':
        return 'Go toolchain label changed'
    if exe_old == exe_new or '/' not in exe_old or '/' not in exe_new:
        return 'unexpected build executable change'
    if exe_old.rsplit('/', 1)[-1] != exe_new.rsplit('/', 1)[-1]:
        return 'build executable name changed'
    for index, (line_old, line_new) in enumerate(zip(before[1:], after[1:]), 1):
        fields_old, fields_new = line_old.split('\t'), line_new.split('\t')
        if len(fields_old) != len(fields_new) or fields_old[1] != fields_new[1]:
            return 'build info field order/key changed at line ' + str(index)
        key = fields_old[1]
        if line_old == line_new:
            continue
        if key == 'mod':
            if (len(fields_old) != 5 or len(fields_new) != 5
                    or fields_old[2] != fields_new[2] or fields_old[4] != fields_new[4]):
                return 'module identity/sum changed in build info'
            old_rev = parse_pseudo_version(fields_old[3])
            new_rev = parse_pseudo_version(fields_new[3])
            if not old_rev or not new_rev:
                return 'module pseudo-version is malformed'
            if (old_rev != context.predecessor_snapshot[:12]
                    or new_rev != context.snapshot[:12]):
                return 'module pseudo-version does not derive from the recorded revision'
        elif key == 'build' and len(fields_old) == 3 and fields_old[2].startswith('vcs.revision='):
            expected_old = 'vcs.revision=' + context.predecessor_snapshot
            expected_new = 'vcs.revision=' + context.snapshot
            if fields_old[2] != expected_old or fields_new[2] != expected_new:
                return 'vcs.revision is not the recorded revision'
        elif key == 'build' and len(fields_old) == 3 and fields_old[2].startswith('vcs.time='):
            if not (parse_utc_time(fields_old[2][len('vcs.time='):])
                    and parse_utc_time(fields_new[2][len('vcs.time='):])):
                return 'vcs.time is not an RFC3339 UTC timestamp'
        else:
            return 'non-volatile build info field changed at line ' + str(index)
    return None


def valid_raw_response(old, new, path, root_old, root_new):
    before = strict_base64(old, path)
    after = strict_base64(new, path)
    lines_old, lines_new = before.split(b'\r\n'), after.split(b'\r\n')
    if len(lines_old) != len(lines_new):
        return 'HTTP wire line count changed'
    changed = 0
    for line_old, line_new in zip(lines_old, lines_new):
        if line_old == line_new:
            continue
        if (not line_old.lower().startswith(b'date: ')
                or not line_new.lower().startswith(b'date: ')):
            return 'non-Date HTTP wire bytes changed'
        old_date = line_old.split(b': ', 1)[1].decode('ascii', 'replace')
        new_date = line_new.split(b': ', 1)[1].decode('ascii', 'replace')
        if not (parse_http_date(old_date) and parse_http_date(new_date)):
            return 'HTTP Date header is malformed'
        changed += 1
    if not changed:
        return 'raw response changed without a Date header difference'
    return None


def valid_stdin(old, new, path, root_old, root_new):
    before, after = strict_base64(old, path), strict_base64(new, path)
    try:
        data_old, data_new = json.loads(before), json.loads(after)
    except (ValueError, UnicodeDecodeError) as error:
        return 'recorded stdin is not JSON: ' + str(error)
    changes = list(compare(data_old, data_new))
    if not changes or any(normalize(item[0]) != '/Config/Port' for item in changes):
        return 'recorded stdin changed outside Config/Port'
    port = changes[0][2]
    if not isinstance(port, int) or isinstance(port, bool) or not 1024 <= port <= 65535:
        return 'recorded SMTP port is not an ephemeral TCP port'
    return None


def valid_stderr(old, new, path, root_old, root_new):
    marker = 'Known providers: '
    before = strict_base64(old, path).decode('utf-8')
    after = strict_base64(new, path).decode('utf-8')
    lines_old, lines_new = before.splitlines(keepends=True), after.splitlines(keepends=True)
    if len(lines_old) != len(lines_new):
        return 'stderr line count changed'
    changed = 0
    for line_old, line_new in zip(lines_old, lines_new):
        if line_old == line_new:
            continue
        if marker not in line_old or marker not in line_new:
            return 'stderr changed outside the provider list'
        if (line_old.split(marker, 1)[0] != line_new.split(marker, 1)[0]
                or not line_old.endswith('\n') or not line_new.endswith('\n')):
            return 'provider diagnostic prefix/suffix changed'
        old_providers = line_old[len(line_old.split(marker, 1)[0]) + len(marker):].strip()
        new_providers = line_new[len(line_new.split(marker, 1)[0]) + len(marker):].strip()
        old_list, new_list = old_providers.split(', '), new_providers.split(', ')
        if (Counter(old_list) != Counter(new_list) or len(old_list) != len(set(old_list))
                or any(not name for name in new_list)):
            return 'provider list is not a pure reordering'
        changed += 1
    if not changed:
        return 'stderr changed without a provider reordering'
    return None


def valid_elapsed(old, new, path, context):
    if (not isinstance(old, int) or not isinstance(new, int)
            or isinstance(old, bool) or isinstance(new, bool) or new < 0):
        return 'elapsed time is not a nonnegative integer'
    return None


def valid_digest_link(old, new, path, root_old, root_new):
    base_path = sibling_path(path, 'sha256')
    if sha256(strict_base64(leaf_at(root_old, base_path), base_path)) != old:
        return 'predecessor digest does not match its base64 bytes'
    if sha256(strict_base64(leaf_at(root_new, base_path), base_path)) != new:
        return 'new digest does not match its base64 bytes'
    return None


RULES = {
    '/source_revision': valid_revision,
    '/archived_generators/Cargo.lock/bytes': valid_lock_bytes,
    '/archived_generators/Cargo.lock/sha256': valid_lock_digest,
    '/embedded_build_info': valid_build_info,
    '/cases/<i>/raw_response_base64': valid_raw_response,
    '/cases/<i>/raw_response/sha256': valid_digest_link,
    '/cases/<i>/stdin_base64': valid_stdin,
    '/cases/<i>/stdin/sha256': valid_digest_link,
    '/observations/helper_limit_controls/<i>/elapsed_ms': valid_elapsed,
    '/cases/<i>/stderr_base64': valid_stderr,
}


def validate_record_diff(old, new, rel, context):
    """Return accepted categories, or raise VerificationError with bad paths."""
    rejections = []
    accepted = Counter()
    for path, old_value, new_value in compare(old, new):
        rule = RULES.get(normalize(path))
        if rule is None:
            rejections.append({'path': rel + path, 'reason': 'not a documented volatile category'})
            continue
        try:
            if rule in (valid_raw_response, valid_stdin, valid_stderr, valid_digest_link):
                reason = rule(old_value, new_value, path, old, new)
            else:
                reason = rule(old_value, new_value, path, context)
        except (VerificationError, KeyError, IndexError, TypeError, ValueError) as error:
            reason = 'validator failed: ' + str(error)
        if reason:
            rejections.append({'path': rel + path, 'reason': reason})
        else:
            accepted[rule.__name__] += 1
    if rejections:
        raise VerificationError('non-whitelisted capture differences', rejections)
    if new.get('source_revision') != context.snapshot:
        raise VerificationError('record source_revision is not the capture revision',
                                [{'path': rel + '/source_revision', 'reason': 'wrong revision'}])
    return dict(accepted)


def load_digests(path, run_id):
    data = json.loads(Path(path).read_text(encoding='utf-8'))
    if str(data.get('run_id', run_id)) != str(run_id):
        raise VerificationError('digest manifest run_id does not match --run-id')
    artifacts = data.get('artifacts')
    if not isinstance(artifacts, list) or len(artifacts) != 3:
        raise VerificationError('digest manifest must list exactly three artifacts')
    result = {}
    for item in artifacts:
        name = item.get('name')
        artifact_id = str(item.get('id', item.get('artifact_id', '')))
        digest = item.get('digest', item.get('sha256', ''))
        target = item.get('target')
        normalized = str(name).lower().replace('_', '-') if name else ''
        if not target:
            for suffix, inferred in TARGET_NAMES.items():
                if normalized == suffix or normalized.endswith('-' + suffix):
                    target = inferred
                    break
        if not name or not artifact_id.isdigit() or target not in SUPPORTED_TARGETS:
            raise VerificationError('invalid artifact entry in digest manifest: ' + repr(item))
        if not isinstance(digest, str) or not digest.startswith('sha256:') or not HEX64.match(digest[7:]):
            raise VerificationError('artifact digest must be sha256:<64 hex>: ' + str(name))
        if name in result:
            raise VerificationError('duplicate artifact name: ' + name)
        result[name] = {'id': artifact_id, 'digest': digest[7:], 'target': target}
    if sorted(entry['target'] for entry in result.values()) != list(SUPPORTED_TARGETS):
        raise VerificationError('digest manifest must cover the three supported targets only')
    return result


def locate_zip(artifacts_dir, name):
    directory = Path(artifacts_dir)
    matches = sorted(p for p in directory.rglob('*.zip') if p.stem == name)
    if len(matches) != 1:
        raise VerificationError('expected exactly one ZIP for artifact ' + name
                                + ', found ' + str(len(matches)))
    return matches[0]


def read_zip(zip_path, expected_digest):
    data = Path(zip_path).read_bytes()
    actual = sha256(data)
    if actual != expected_digest:
        raise VerificationError('ZIP digest mismatch for ' + str(zip_path) + ': '
                                + actual + ' != ' + expected_digest)
    try:
        archive = zipfile.ZipFile(zip_path)
    except zipfile.BadZipFile as error:
        raise VerificationError('invalid artifact ZIP ' + str(zip_path) + ': ' + str(error))
    identities, records = [], {}
    with archive:
        for member in archive.namelist():
            name = member.replace('\\', '/')
            if name.startswith('/') or '..' in parts(name):
                raise VerificationError('unsafe ZIP member: ' + name)
            if name.endswith('/'):
                continue
            payload = archive.read(member)
            if Path(name).name == 'capture-identity.json':
                identities.append(json.loads(payload))
                continue
            components = parts(name)
            prefixes = [i for i, item in enumerate(components)
                        if item.startswith(RECORD_PREFIX)]
            if not prefixes:
                continue
            if len(prefixes) != 1 or prefixes[0] != 0 or not name.endswith('.json'):
                raise VerificationError('unexpected record member: ' + name)
            key = '/'.join(components)
            if key in records:
                raise VerificationError('duplicate record member: ' + key)
            records[key] = payload
    if len(identities) != 1:
        raise VerificationError('expected one capture-identity.json in ' + str(zip_path))
    return identities[0], records, actual


def validate_identity(identity, candidate, target, snapshot_holder):
    if not isinstance(identity, dict):
        raise VerificationError('capture identity is not an object')
    snapshot = identity.get('capture_revision')
    if not HEX40.match(str(snapshot)):
        raise VerificationError('capture identity has no valid capture_revision')
    if identity.get('product_candidate_revision') != candidate:
        raise VerificationError('capture identity product candidate mismatch')
    if identity.get('retirement_revision') != RETIREMENT:
        raise VerificationError('capture identity retirement revision mismatch')
    go = identity.get('go') or {}
    if (go.get('GOVERSION') != GO_VERSION
            or (go.get('GOHOSTOS'), go.get('GOHOSTARCH')) != tuple(target.split('/'))):
        raise VerificationError('capture identity native target/toolchain mismatch: ' + target)
    if 'no frozen-reader' not in str(identity.get('scope', '')):
        raise VerificationError('capture identity scope is not capture-only')
    if snapshot_holder[0] is None:
        snapshot_holder[0] = snapshot
    elif snapshot_holder[0] != snapshot:
        raise VerificationError('artifacts disagree on capture_revision')
    return snapshot


def fixture_rel(record_key, target):
    name = record_key[len(RECORD_PREFIX):]
    family, filename = name.split('/', 1)
    return family + '/' + target.replace('/', '-') + '/' + filename


def load_predecessor(repo, refresh):
    root = Path(repo) / FIXTURE_ROOT / ('refresh-' + refresh)
    if not root.is_dir():
        raise VerificationError('missing predecessor fixture directory: ' + str(root))
    records = {}
    for path in sorted(root.rglob('*.json')):
        records[path.relative_to(root).as_posix()] = path.read_bytes()
    if not records:
        raise VerificationError('predecessor fixture directory has no records')
    return root, records


def candidate_lock(repo, candidate):
    data = git(repo, 'cat-file', '--filters', candidate + ':Cargo.lock')
    return len(data), sha256(data)


def validate_provenance(old_bytes, new_bytes, rel, target, context, snapshot):
    try:
        old, new = json.loads(old_bytes), json.loads(new_bytes)
    except ValueError as error:
        raise VerificationError('record is not JSON: ' + rel + ': ' + str(error))
    if not isinstance(old, dict) or not isinstance(new, dict):
        raise VerificationError('record is not a JSON object: ' + rel,
                                [{'path': rel, 'reason': 'record structure changed'}])
    if set(old) != set(new):
        changed = sorted(set(old) ^ set(new))
        raise VerificationError('record top-level keys changed: ' + rel,
                                [{'path': rel + '/' + key,
                                  'reason': 'record structure changed'} for key in changed])
    for key in ('schema', 'source_files', 'scope', 'native_target', 'go_version',
                'checkout_autocrlf', 'family', 'test_file', 'test'):
        if old.get(key) != new.get(key):
            raise VerificationError('record provenance field changed: ' + rel + '/' + key)
    if new.get('native_target') != target or new.get('go_version') != GO_VERSION:
        raise VerificationError('record target/toolchain mismatch: ' + rel)
    if set(old.get('archived_generators', {})) != set(new.get('archived_generators', {})):
        raise VerificationError('archived generator set changed: ' + rel)
    for generator, entry in old.get('archived_generators', {}).items():
        current = new['archived_generators'][generator]
        if generator == 'Cargo.lock':
            continue
        if entry != current:
            raise VerificationError('archived generator bytes changed: ' + rel
                                    + '/archived_generators/' + generator)
    accepted = validate_record_diff(old, new, rel, context)
    return new, accepted


def repoint_readers(repo, import_root, predecessor_refresh, refresh,
                    old_snapshot, new_snapshot, predecessor_records, new_records):
    old_prefix = 'refresh-' + predecessor_refresh + '/'
    new_prefix = 'refresh-' + refresh + '/'
    changed = []
    for reader in READERS:
        source = Path(repo) / reader
        destination = Path(import_root) / reader
        if not destination.exists():
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
        text = destination.read_text(encoding='utf-8')
        if old_snapshot not in text:
            raise VerificationError('reader lacks the predecessor revision anchor: ' + reader)
        lines = text.splitlines(keepends=True)
        replacements = []
        includes = 0
        for index, line in enumerate(lines):
            for match in re.finditer(re.escape(old_prefix) + r'([^\s"]+\.json)', line):
                includes += 1
                relative = match.group(1)
                if relative not in new_records:
                    raise VerificationError('reader references an unknown record: '
                                            + reader + '/' + relative)
                old_digest = sha256(predecessor_records[relative])
                new_digest = sha256(new_records[relative])
                candidates = []
                for window in range(max(0, index - 2), min(len(lines), index + 3)):
                    for token in TOKEN64.finditer(lines[window]):
                        candidates.append((window, token.start(), token.end(), token.group()))
                matching = [item for item in candidates if item[3] == old_digest]
                if len(matching) != 1:
                    raise VerificationError('digest anchor is not unique for ' + reader
                                            + '/' + relative)
                window, start, end, _ = matching[0]
                replacements.append((window, start, end, new_digest))
        if not includes:
            raise VerificationError('reader has no predecessor fixture includes: ' + reader)
        per_line = {}
        for window, start, end, digest in replacements:
            per_line.setdefault(window, []).append((start, end, digest))
        for window, edits in per_line.items():
            for start, end, digest in sorted(edits, reverse=True):
                lines[window] = lines[window][:start] + digest + lines[window][end:]
        updated = ''.join(lines).replace(old_snapshot, new_snapshot).replace(old_prefix, new_prefix)
        for relative, data in new_records.items():
            if sha256(predecessor_records[relative]) != sha256(data) and sha256(predecessor_records[relative]) in updated:
                raise VerificationError('stale predecessor digest anchor in ' + reader)
        if new_snapshot not in updated or updated.count(new_prefix) != includes:
            raise VerificationError('reader repointing did not produce a complete update: ' + reader)
        if updated != text:
            destination.write_text(updated, encoding='utf-8')
            changed.append(reader)
    return changed


def write_report(path, payload):
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(payload, indent=2, sort_keys=True) + '\n',
                           encoding='utf-8')


def import_refresh(repo, artifacts_dir, digests_path, run_id, candidate,
                   predecessor_refresh, import_root, report_path, snapshot=None):
    repo = Path(repo).resolve()
    candidate = git(repo, 'rev-parse', '--verify', candidate + '^{commit}').decode().strip()
    if not HEX40.match(candidate):
        raise VerificationError('candidate does not resolve to a 40-hex commit')
    if HEX8.match(predecessor_refresh):
        predecessor_refresh = predecessor_refresh
    elif re.fullmatch(r'refresh-[0-9a-f]{8}', predecessor_refresh):
        predecessor_refresh = predecessor_refresh[len('refresh-'):]
    else:
        raise VerificationError('predecessor refresh must be an 8-hex id')
    refresh = candidate[:8]
    digests = load_digests(digests_path, run_id)
    predecessor_root, predecessor_records = load_predecessor(repo, predecessor_refresh)
    predecessor_snapshot = set(json.loads(data)['source_revision']
                               for data in predecessor_records.values())
    if len(predecessor_snapshot) != 1:
        raise VerificationError('predecessor records disagree on source_revision')
    predecessor_snapshot = predecessor_snapshot.pop()
    lock_bytes, lock_digest = candidate_lock(repo, candidate)
    context = CheckContext(snapshot or '', predecessor_snapshot, candidate,
                           lock_bytes, lock_digest)
    snapshot_holder = [snapshot]
    records = {}
    artifacts_report = []
    for name, entry in sorted(digests.items()):
        zip_path = locate_zip(artifacts_dir, name)
        identity, members, actual = read_zip(zip_path, entry['digest'])
        current_snapshot = validate_identity(identity, candidate, entry['target'], snapshot_holder)
        context.snapshot = snapshot_holder[0]
        artifacts_report.append({
            'id': entry['id'], 'name': name, 'target': entry['target'],
            'sha256': actual, 'zip': str(zip_path),
            'capture_revision': current_snapshot,
        })
        for key, data in members.items():
            rel = fixture_rel(key, entry['target'])
            if rel in records:
                raise VerificationError('duplicate fixture path across artifacts: ' + rel)
            records[rel] = data
    snapshot = snapshot_holder[0]
    context.snapshot = snapshot
    if set(records) != set(predecessor_records):
        raise VerificationError('record inventory differs from predecessor: missing='
                                + repr(sorted(set(predecessor_records) - set(records)))
                                + ' extra=' + repr(sorted(set(records) - set(predecessor_records))))
    if len(records) != 48:
        raise VerificationError('expected 48 records, found ' + str(len(records)))

    accepted = Counter()
    verified = {}
    for rel in sorted(records):
        target = parts(rel)[1].replace('-', '/', 1)
        new, categories = validate_provenance(predecessor_records[rel], records[rel], rel,
                                              target, context, snapshot)
        if new.get('source_revision') != snapshot:
            raise VerificationError('record source_revision mismatch: ' + rel)
        verified[rel] = records[rel]
        for category, count in categories.items():
            accepted[category] += count

    root = Path(import_root).resolve()
    if root == repo:
        raise VerificationError('import-root must be a separate worktree; it is never '
                                'written in place by this tool')
    destination = root / FIXTURE_ROOT / ('refresh-' + refresh)
    if destination.exists() and any(destination.rglob('*')):
        raise VerificationError('destination fixture directory already exists: ' + str(destination))
    destination.mkdir(parents=True, exist_ok=True)
    for rel, data in sorted(verified.items()):
        path = destination / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        if not path.read_bytes().endswith(data) or sha256(path.read_bytes()) != sha256(data):
            raise VerificationError('fixture copy is not byte-identical: ' + rel)
    readers = repoint_readers(repo, root, predecessor_refresh, refresh,
                              predecessor_snapshot, snapshot,
                              predecessor_records, verified)
    payload = {
        'status': 'accepted',
        'tool': 'import_capture_refresh',
        'run_id': str(run_id),
        'candidate': candidate,
        'snapshot': snapshot,
        'predecessor_snapshot': predecessor_snapshot,
        'predecessor_refresh': predecessor_refresh,
        'refresh': refresh,
        'artifacts': artifacts_report,
        'record_count': len(verified),
        'accepted_differences': dict(sorted(accepted.items())),
        'documented_categories': list(CATEGORIES),
        'fixture_root': (destination.relative_to(root)).as_posix(),
        'fixtures': sorted(verified),
        'readers': readers,
    }
    write_report(report_path, payload)
    return payload


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', required=True, help='GitHub Actions capture run ID')
    parser.add_argument('--artifacts-dir', required=True,
                        help='directory containing the three downloaded artifact ZIPs')
    parser.add_argument('--digests-json', required=True,
                        help='JSON produced by the coordinator with gh api artifact digests')
    parser.add_argument('--candidate', required=True, help='product candidate commit SHA')
    parser.add_argument('--predecessor-refresh', required=True,
                        help='previous refresh id, e.g. a8eda4f7')
    parser.add_argument('--repo', default=str(Path(__file__).resolve().parent.parent),
                        help='source repository (read-only)')
    parser.add_argument('--import-root', required=True,
                        help='separate worktree that receives fixtures and reader edits')
    parser.add_argument('--report', help='report path; defaults to the artifacts directory')
    parser.add_argument('--snapshot',
                        help='expected capture snapshot SHA; otherwise taken from artifacts')
    args = parser.parse_args(argv)
    report = args.report or str(Path(args.artifacts_dir) / 'capture-refresh-import-report.json')
    try:
        payload = import_refresh(args.repo, args.artifacts_dir, args.digests_json,
                                 args.run_id, args.candidate, args.predecessor_refresh,
                                 args.import_root, report, args.snapshot)
    except VerificationError as error:
        if error.report is None:
            error.report = {'status': 'rejected', 'reason': str(error),
                            'rejections': error.rejections}
        write_report(report, error.report)
        print('import_capture_refresh: ' + str(error), file=sys.stderr)
        for item in error.rejections:
            print('  ' + item['path'] + ': ' + item['reason'], file=sys.stderr)
        return 2
    print(json.dumps({'status': 'accepted', 'records': payload['record_count'],
                      'snapshot': payload['snapshot'], 'refresh': payload['refresh'],
                      'report': report}, sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
