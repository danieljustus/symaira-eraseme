#!/usr/bin/env python3
"""Run pinned official MCP core scenarios against the real Rust loopback server.

The product advertises tools only. Other upstream scenarios require reference
test_* tools, resources, prompts, logging, sampling or SSE features outside its
pinned catalogue. This is the applicable core subset, not the full upstream suite.
This Unix harness does not substitute for the six native HTTP platform gates.
No Go executable is looked up, built or compared; only the Rust binary is run.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import time

OFFICIAL_REVISION = "21a9a2febd7100d7c17ac1021ee7f2ed9f66a1e0"
LOCK_SHA256 = "df89d138b91871a7fb041f8d3923a78b9a1e2d5bdf28ba589fe6f944f30814fe"
SCENARIOS = ("server-initialize", "ping", "tools-list")
SCOPE = ("Rust-only official applicable core subset; no Go executable, baseline, "
         "rewritten assertions or reference tools")
ROOT = Path(__file__).resolve().parent.parent


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    # Not assert: python -O strips asserts and would silently pass acceptance.
    if not condition:
        raise RuntimeError(message)


def require_executable(path):
    require(path.is_file() and os.access(path, os.X_OK), f'Rust binary is not an executable file: {path}')


def verify_results(results):
    """Acceptance over the three Rust official scenario results."""
    require(all(r.get('binary') == 'rust' for r in results), 'Non-Rust result in Rust-only conformance')
    scenarios = [r.get('scenario') for r in results]
    require(scenarios == list(SCENARIOS), f'Official scenario inventory mismatch: {scenarios}')
    for r in results:
        require(r.get('exit_status') == 0, f"{r['scenario']} exited with {r.get('exit_status')}")
        checks = r.get('checks')
        require(isinstance(checks, list) and checks, f"{r['scenario']} produced zero official checks")
        failed = [c for c in checks if not isinstance(c, dict) or c.get('status') != 'SUCCESS']
        require(not failed, f"{r['scenario']} has non-SUCCESS checks: {failed}")


def build_manifest(source_revision, sdk_version, node_version, rust_path, results, captured_at):
    return dict(source_revision=source_revision, official_revision=OFFICIAL_REVISION,
                official_version='0.1.16', official_lock_sha256=LOCK_SHA256,
                sdk_version=sdk_version, node_version=node_version, implementation='rust-only',
                auth_adapter_sha256=sha(ROOT / 'scripts/mcp-conformance-auth.mjs'),
                binary_sha256={'rust': sha(rust_path)}, captured_at_utc=captured_at,
                scope=SCOPE, results=results)


def bounded(args, cwd, env, stem, seconds):
    """Bound output and time; always reap the owned process group."""
    with stem.with_suffix('.stdout').open('xb') as out, stem.with_suffix('.stderr').open('xb') as err:
        proc = subprocess.Popen(args, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                stdout=out, stderr=err, start_new_session=True)
        try:
            deadline = time.monotonic() + seconds
            while proc.poll() is None:
                if time.monotonic() >= deadline:
                    raise RuntimeError('Owned conformance process exceeded deadline')
                if any(p.stat().st_size > 4 * 1024 * 1024 for p in
                       (stem.with_suffix('.stdout'), stem.with_suffix('.stderr'))):
                    raise RuntimeError('Owned conformance process exceeded output limit')
                time.sleep(0.025)
            return proc.returncode
        finally:
            # Reap descendants too, even when the parent has already exited.
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait(timeout=2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--conformance', type=Path, required=True)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if os.name != 'posix':
        raise RuntimeError('This harness requires Unix owned process groups')
    official = args.conformance.resolve()
    rust = args.rust.resolve()
    node = shutil.which('node')
    require(node is not None, 'Node is required for the official client')
    require_executable(rust)
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    require(not subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT), 'Harness checkout is not clean')
    require(subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=official, text=True).strip() == OFFICIAL_REVISION,
            'Official source revision mismatch')
    require(not subprocess.check_output(['git', 'status', '--porcelain'], cwd=official), 'Official checkout is not clean')
    require(sha(official / 'package-lock.json') == LOCK_SHA256, 'Official lockfile digest mismatch')
    require(json.loads((official / 'package.json').read_text())['version'] == '0.1.16', 'Official version mismatch')
    sdk = json.loads((official / 'node_modules/@modelcontextprotocol/sdk/package.json').read_text())['version']
    require(sdk == '1.27.1', 'Official SDK version mismatch')
    results = []
    args.output.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(prefix='mcp-conformance-') as temp:
        private = Path(temp)
        root = private / 'rust'
        root.mkdir(mode=0o700)
        for folder in ('data', 'config', 'cache', 'state', 'temp'):
            (root / folder).mkdir(mode=0o700)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        runtime = dict(HOME=str(root), USERPROFILE=str(root), TZ='UTC',
                       SYMERASEME_DATA_DIR=str(root / 'data'),
                       TMPDIR=str(root / 'temp'), TMP=str(root / 'temp'), TEMP=str(root / 'temp'),
                       **{f'XDG_{key}_HOME': str(root / folder) for key, folder in
                          (('CONFIG', 'config'), ('CACHE', 'cache'), ('DATA', 'data'), ('STATE', 'state'))})
        with (root / 'server.stdout').open('xb') as stdout, (root / 'server.stderr').open('xb') as stderr:
            server = subprocess.Popen([str(rust), 'mcp', '--host', '127.0.0.1', '--port', str(port)], cwd=root, env=runtime, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True)
            try:
                deadline = time.monotonic() + 10
                while True:
                    require(server.poll() is None, 'Owned server exited before readiness')
                    require(time.monotonic() < deadline, 'Owned server readiness deadline')
                    if (root / 'data/mcp_token').is_file():
                        try:
                            with socket.create_connection(('127.0.0.1', port), timeout=0.1):
                                break
                        except OSError:
                            pass
                    time.sleep(0.01)
                token = (root / 'data/mcp_token').read_text().strip()
                require(len(token) == 43, 'Owned server token has unexpected length')
                endpoint = f'http://127.0.0.1:{port}/'
                env = dict(PATH=os.environ['PATH'], HOME=str(root), USERPROFILE=str(root), TZ='UTC', MCP_PROBE_ENDPOINT=endpoint, MCP_PROBE_BEARER=token)
                for scenario in SCENARIOS:
                    destination = root / (scenario + '-results')
                    status = bounded([node, '--import', str(ROOT / 'scripts/mcp-conformance-auth.mjs'), str(official / 'dist/index.js'), 'server', '--url', endpoint, '--scenario', scenario, '--output-dir', str(destination), '--verbose'], root, env, root / scenario, 30)
                    files = list(destination.rglob('checks.json'))
                    require(len(files) == 1, f'{scenario} did not produce exactly one checks.json')
                    checks = json.loads(files[0].read_text())
                    results.append(dict(binary='rust', scenario=scenario, exit_status=status, checks=checks))
                    print('rust', scenario, status, [c['status'] for c in checks], flush=True)
            finally:
                try:
                    os.killpg(server.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    server.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    os.killpg(server.pid, signal.SIGKILL)
                    server.wait(timeout=2)
                try:
                    os.killpg(server.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
    manifest = build_manifest(revision, sdk, subprocess.check_output([node, '--version'], text=True).strip(),
                              rust, results, datetime.datetime.now(datetime.timezone.utc).isoformat())
    (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    verify_results(results)


if __name__ == '__main__':
    main()
