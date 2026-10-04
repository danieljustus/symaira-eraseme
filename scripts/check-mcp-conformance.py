#!/usr/bin/env python3
"""Run pinned official MCP core scenarios against real Go/Rust loopback servers.

The product advertises tools only. Other upstream scenarios require reference
test_* tools, resources, prompts, logging, sampling or SSE features outside its
pinned catalogue. This is the applicable core subset, not the full upstream suite.
This Unix harness does not substitute for the six native HTTP platform gates.
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
ROOT = Path(__file__).resolve().parent.parent


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
    go, node = shutil.which('go'), shutil.which('node')
    assert go and node and rust.is_file()
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)
    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=official, text=True).strip() == OFFICIAL_REVISION
    assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=official)
    assert sha(official / 'package-lock.json') == LOCK_SHA256
    assert json.loads((official / 'package.json').read_text())['version'] == '0.1.16'
    sdk = json.loads((official / 'node_modules/@modelcontextprotocol/sdk/package.json').read_text())['version']
    assert sdk == '1.27.1'
    info = json.loads(subprocess.check_output([go, 'env', '-json', 'GOVERSION', 'GOCACHE', 'GOMODCACHE'], timeout=10))
    assert info['GOVERSION'] == 'go1.26.6'
    results = []
    args.output.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(prefix='mcp-conformance-') as temp:
        private = Path(temp)
        build_env = dict(PATH=os.environ['PATH'], HOME=temp, USERPROFILE=temp,
                         GOENV='off', GOWORK='off', GOTOOLCHAIN='local', GOPROXY='off',
                         GOSUMDB='off', GOCACHE=info['GOCACHE'], GOMODCACHE=info['GOMODCACHE'],
                         CGO_ENABLED='0', TZ='UTC')
        go_binary = private / 'symeraseme-go'
        assert bounded([go, 'build', '-mod=readonly', '-buildvcs=true', '-o', str(go_binary), './cmd/symeraseme'], ROOT, build_env, private / 'build', 120) == 0
        build_info = subprocess.check_output([go, 'version', '-m', str(go_binary)], env=build_env, timeout=10).decode()
        assert 'vcs.revision=' + revision in build_info and 'vcs.modified=false' in build_info
        binaries = {'go': go_binary, 'rust': rust}
        for name, binary in binaries.items():
            root = private / name
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
                server = subprocess.Popen([str(binary), 'mcp', '--host', '127.0.0.1', '--port', str(port)], cwd=root, env=runtime, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True)
                try:
                    deadline = time.monotonic() + 10
                    while True:
                        assert server.poll() is None, 'Owned server exited before readiness'
                        assert time.monotonic() < deadline, 'Owned server readiness deadline'
                        if (root / 'data/mcp_token').is_file():
                            try:
                                with socket.create_connection(('127.0.0.1', port), timeout=0.1):
                                    break
                            except OSError:
                                pass
                        time.sleep(0.01)
                    token = (root / 'data/mcp_token').read_text().strip()
                    assert len(token) == 43
                    endpoint = f'http://127.0.0.1:{port}/'
                    env = dict(PATH=os.environ['PATH'], HOME=str(root), USERPROFILE=str(root), TZ='UTC', MCP_PROBE_ENDPOINT=endpoint, MCP_PROBE_BEARER=token)
                    for scenario in SCENARIOS:
                        destination = root / (scenario + '-results')
                        status = bounded([node, '--import', str(ROOT / 'scripts/mcp-conformance-auth.mjs'), str(official / 'dist/index.js'), 'server', '--url', endpoint, '--scenario', scenario, '--output-dir', str(destination), '--verbose'], root, env, root / scenario, 30)
                        files = list(destination.rglob('checks.json'))
                        assert len(files) == 1
                        checks = json.loads(files[0].read_text())
                        results.append(dict(binary=name, scenario=scenario, exit_status=status, checks=checks))
                        print(name, scenario, status, [c['status'] for c in checks], flush=True)
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
        manifest = dict(source_revision=revision, official_revision=OFFICIAL_REVISION,
                        official_version='0.1.16', official_lock_sha256=LOCK_SHA256,
                        sdk_version=sdk, node_version=subprocess.check_output([node, '--version'], text=True).strip(),
                        auth_adapter_sha256=sha(ROOT / 'scripts/mcp-conformance-auth.mjs'),
                        go_build_info=build_info, binary_sha256={n: sha(p) for n, p in binaries.items()},
                        captured_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                        scope='Official applicable core subset; no baseline, rewritten assertions or reference tools', results=results)
        (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    assert len(results) == 6
    assert all(r['exit_status'] == 0 and r['checks'] and
               all(c['status'] == 'SUCCESS' for c in r['checks']) for r in results), 'Official core conformance failed'


if __name__ == '__main__':
    main()
