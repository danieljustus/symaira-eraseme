#!/usr/bin/env python3
"""Artifact verification controls and a replay against genuine captured records."""
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    'import_capture_refresh', ROOT / 'scripts/import_capture_refresh.py')
if SPEC is None or SPEC.loader is None:
    raise RuntimeError('Cannot load capture refresh importer')
IMPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMPORT)

CANDIDATE = '5f5b27a2802395596dbb59b0362bc7fd71ed9b4a'
SNAPSHOT = '17c7d69658eeb22c4ac96a3c0d4facfb6a372274'
PRED_SNAPSHOT = 'a8eda4f77ce938c32d18dbad7d45de148b7d0482'
PRED_REFRESH = 'a8eda4f7'
# Squash commit of #1344 on main: imports refresh-5f5b27a2 over refresh-a8eda4f7.
IMPORT_COMMIT = '3aa50350d031ec6f27042e097001d4ee26136def'
FIXTURES = ROOT / 'tests/fixtures/go-frozen'
READERS = IMPORT.READERS
ARTIFACTS = (
    ('native-dependency-captures-macOS-arm64', 'darwin/arm64', '15173993674'),
    ('native-dependency-captures-Linux-amd64', 'linux/amd64', '15173656654'),
    ('native-dependency-captures-Linux-arm64', 'linux/arm64', '15173696597'),
)


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])


def make_artifacts(temporary, source=FIXTURES / 'refresh-5f5b27a2', mutate=None,
                   omit=None, tamper_digest=False):
    """Build genuine-shaped artifact ZIPs and a coordinator digest manifest."""
    directory = Path(temporary)
    artifacts = directory / 'artifacts'
    artifacts.mkdir()
    identity = {
        'capture_revision': SNAPSHOT, 'product_candidate_revision': CANDIDATE,
        'retirement_revision': IMPORT.RETIREMENT, 'go': {'GOVERSION': 'go1.26.6'},
        'workspace_root': '/runner/work', 'cargo_target_directory': '/runner/target',
        'checkout_autocrlf': 'false', 'native_host_agent_sha256': '0' * 64,
        'scope': 'Actual Go captures only; no frozen-reader, product-native or release acceptance.',
    }
    manifest = []
    for name, target, artifact_id in ARTIFACTS:
        operating_system, architecture = target.split('/')
        current = dict(identity)
        current['go'] = {
            'GOVERSION': 'go1.26.6', 'GOHOSTOS': operating_system,
            'GOHOSTARCH': architecture, 'GOOS': operating_system, 'GOARCH': architecture,
        }
        path = artifacts / (name + '.zip')
        with zipfile.ZipFile(path, 'w') as archive:
            archive.writestr('capture-identity.json', json.dumps(current))
            for record in sorted(source.rglob('*.json')):
                relative = record.relative_to(source).as_posix()
                family, target_dir, case = relative.split('/')
                if target_dir != target.replace('/', '-'):
                    continue
                if omit and relative == omit:
                    continue
                data = record.read_bytes()
                if mutate and relative == mutate[0]:
                    data = mutate[1](data)
                archive.writestr('frozen-go-' + family + '/' + case, data)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if tamper_digest:
            digest = '0' * 64
        manifest.append({'id': artifact_id, 'name': name, 'target': target,
                         'digest': 'sha256:' + digest})
    digests = directory / 'digests.json'
    digests.write_text(json.dumps({'run_id': '45943116789', 'artifacts': manifest}),
                       encoding='utf-8')
    return artifacts, digests


def committed_reader(reader):
    """Reader bytes right after the refresh-5f5b27a2 import landed on main.

    Later refreshes repoint the readers in the working tree, so the replay
    compares against this fixed commit instead of the current checkout.
    """
    return git('show', IMPORT_COMMIT + ':' + reader)


def prepare_import_root(temporary):
    """Seed a destination worktree with the reader state before that import."""
    destination = Path(temporary) / 'import'
    destination.mkdir()
    for reader in READERS:
        target = destination / reader
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            target.write_bytes(git('show', IMPORT_COMMIT + '^:' + reader))
        except subprocess.CalledProcessError:
            # Fallback for shallow checkouts: reverse the committed import text.
            text = committed_reader(reader).decode('utf-8')
            for record in (FIXTURES / 'refresh-5f5b27a2').rglob('*.json'):
                relative = record.relative_to(FIXTURES / 'refresh-5f5b27a2').as_posix()
                predecessor = FIXTURES / 'refresh-a8eda4f7' / relative
                text = text.replace(hashlib.sha256(record.read_bytes()).hexdigest(),
                                    hashlib.sha256(predecessor.read_bytes()).hexdigest())
            text = text.replace(SNAPSHOT, PRED_SNAPSHOT)
            text = text.replace('refresh-5f5b27a2/', 'refresh-a8eda4f7/')
            target.write_text(text, encoding='utf-8')
    return destination


class CaptureRefreshImport(unittest.TestCase):
    def test_genuine_replay_reproduces_committed_reader_and_fixture_bytes(self):
        with tempfile.TemporaryDirectory(prefix='capture-import-test-') as temporary:
            artifacts, digests = make_artifacts(temporary)
            destination = prepare_import_root(temporary)
            report = Path(temporary) / 'report.json'
            payload = IMPORT.import_refresh(
                ROOT, artifacts, digests, '45943116789', CANDIDATE, PRED_REFRESH,
                destination, report, SNAPSHOT)
            self.assertEqual(payload['record_count'], 48)
            self.assertEqual(payload['snapshot'], SNAPSHOT)
            self.assertEqual(set(payload['readers']), set(READERS))
            for category in ('valid_revision', 'valid_build_info', 'valid_lock_bytes',
                             'valid_lock_digest', 'valid_raw_response', 'valid_stdin',
                             'valid_stderr', 'valid_elapsed', 'valid_digest_link'):
                self.assertIn(category, payload['accepted_differences'])
            self.assertTrue(report.is_file())
            for record in (FIXTURES / 'refresh-5f5b27a2').rglob('*.json'):
                relative = record.relative_to(FIXTURES / 'refresh-5f5b27a2')
                copied = destination / 'tests/fixtures/go-frozen/refresh-5f5b27a2' / relative
                self.assertEqual(copied.read_bytes(), record.read_bytes())
            for reader in READERS:
                self.assertEqual((destination / reader).read_bytes(),
                                 committed_reader(reader))

    def test_non_whitelisted_json_path_stops_the_import(self):
        with tempfile.TemporaryDirectory(prefix='capture-import-test-') as temporary:
            rel = 'agent-error/darwin-arm64/agent-error.json'

            def inject(data):
                record = json.loads(data)
                record['unexpected'] = 1
                return json.dumps(record).encode('utf-8')

            artifacts, digests = make_artifacts(temporary, mutate=(rel, inject))
            destination = prepare_import_root(temporary)
            with self.assertRaises(IMPORT.VerificationError) as caught:
                IMPORT.import_refresh(ROOT, artifacts, digests, '45943116789', CANDIDATE,
                                      PRED_REFRESH, destination,
                                      Path(temporary) / 'rejected.json', SNAPSHOT)
            rejection_text = ' '.join(item['path'] for item in caught.exception.rejections)
            self.assertIn('/unexpected', rejection_text)
            self.assertFalse((destination / 'tests/fixtures').exists())

    def test_api_digest_mismatch_and_incomplete_inventory_are_rejected(self):
        for label, options in (
            ('digest', {'tamper_digest': True}),
            ('inventory', {'omit': 'smtp/linux-arm64/transport.json'}),
        ):
            with self.subTest(label=label):
                with tempfile.TemporaryDirectory(prefix='capture-import-test-') as temporary:
                    artifacts, digests = make_artifacts(temporary, **options)
                    destination = prepare_import_root(temporary)
                    with self.assertRaises(IMPORT.VerificationError) as caught:
                        IMPORT.import_refresh(ROOT, artifacts, digests, '45943116789',
                                              CANDIDATE, PRED_REFRESH, destination,
                                              Path(temporary) / 'rejected.json', SNAPSHOT)
                    self.assertIn(label, str(caught.exception).lower()
                                  if label == 'digest' else str(caught.exception))

    def test_comparator_override_digest_is_the_only_generator_change(self):
        # #1335: a retired comparator's archived digest may move only from its
        # 22615ae entry to the entry with the reviewed chrono edits applied.
        candidate = git('rev-parse', 'HEAD').decode().strip()
        overrides = IMPORT.generator_overrides(ROOT, candidate)
        self.assertEqual(IMPORT.generator_overrides(ROOT, CANDIDATE), {})
        name = 'crates/symeraseme-core/tests/campaign_smtp.rs'
        rel = 'smtp/darwin-arm64/campaign.json'
        old_bytes = (FIXTURES / 'refresh-711fd570' / rel).read_bytes()
        old = json.loads(old_bytes)
        retired, expected = overrides[name]
        self.assertEqual(old['archived_generators'][name], retired)
        revision = old['source_revision']
        context = IMPORT.CheckContext(revision, revision, candidate, 0, '', overrides)
        new = json.loads(old_bytes)
        new['archived_generators'][name] = expected
        _, accepted = IMPORT.validate_provenance(old_bytes, json.dumps(new).encode('utf-8'),
                                                 rel, 'darwin/arm64', context, revision)
        self.assertEqual(accepted, {'comparator_override': 1})
        new['archived_generators'][name] = {'bytes': expected['bytes'], 'sha256': 'f' * 64}
        with self.assertRaisesRegex(IMPORT.VerificationError, '#1335'):
            IMPORT.validate_provenance(old_bytes, json.dumps(new).encode('utf-8'),
                                       rel, 'darwin/arm64', context, revision)

    def test_typed_validators_reject_out_of_category_changes(self):
        context = IMPORT.CheckContext(SNAPSHOT, PRED_SNAPSHOT, CANDIDATE, 80218, 'a' * 64)
        self.assertIsNotNone(IMPORT.valid_build_info(
            '/tmp/.tmpX/tool: go1.26.6\n\tpath\tx\n\tdep\tpkg\tv1\told\n',
            '/tmp/.tmpY/tool: go1.26.6\n\tpath\tx\n\tdep\tpkg\tv2\told\n',
            '/embedded_build_info', context))
        http_old = base64.b64encode(b'HTTP/1.1 200 OK\r\nDate: Mon, 23 Nov 2026 13:50:35 GMT\r\n\r\n').decode('ascii')
        http_new = base64.b64encode(b'HTTP/1.1 400 Bad Request\r\nDate: Mon, 23 Nov 2026 13:50:35 GMT\r\n\r\n').decode('ascii')
        self.assertIsNotNone(IMPORT.valid_raw_response(http_old, http_new,
                                                       '/cases/0/raw_response_base64', {}, {}))
        stdin_old = base64.b64encode(json.dumps({'Config': {'Port': 4200, 'Host': 'x'}}).encode()).decode('ascii')
        stdin_new = base64.b64encode(json.dumps({'Config': {'Port': 4201, 'Host': 'y'}}).encode()).decode('ascii')
        self.assertIsNotNone(IMPORT.valid_stdin(stdin_old, stdin_new,
                                                '/cases/0/stdin_base64', {}, {}))
        stderr_old = base64.b64encode(b'Known providers: openai, anthropic\n').decode('ascii')
        stderr_new = base64.b64encode(b'Known providers: openai, anthropic, agent\n').decode('ascii')
        self.assertIsNotNone(IMPORT.valid_stderr(stderr_old, stderr_new,
                                                 '/cases/0/stderr_base64', {}, {}))
        self.assertIsNotNone(IMPORT.valid_elapsed(10, -1, '/observations/x/0/elapsed_ms', context))
        self.assertIsNone(IMPORT.valid_elapsed(10, 11, '/observations/x/0/elapsed_ms', context))


if __name__ == '__main__':
    unittest.main()
