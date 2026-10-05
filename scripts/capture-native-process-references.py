#!/usr/bin/env python3
"""Run original native Go/Rust comparators before validating frozen readers.

This producer retains genuine observations even when a Go source change makes
older frozen references fail. The complete workspace suite and all frozen
reader checks still run separately and must pass before acceptance.
"""
import argparse
import os
import pathlib
import re
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-root', type=pathlib.Path, required=True)
    args = parser.parse_args()
    source = pathlib.Path(__file__).resolve().parent.parent
    assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=source)
    assert args.output_root.is_absolute()
    output = args.output_root.resolve()
    assert not output.is_relative_to(source)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment['SYMERASEME_PARITY_LIVE_GO'] = '1'
    for key, name in [
        ('PLAN_PROCESSES', 'plan-processes'), ('HTTP_WIRE', 'http-wire'),
        ('HTTP_HEADERS', 'http-headers'), ('SMTP', 'smtp'),
        ('AGENT_ERROR', 'agent-error'), ('NATIVE_TRIAGE', 'native-triage'),
    ]:
        destination = output / ('frozen-go-' + name)
        assert not destination.exists(), 'capture must never replace a prior measurement'
        environment['SYMERASEME_CAPTURE_' + key] = str(destination)
    if os.name != 'nt':
        os.umask(0o022)
    commands = [
        ('plan', 'symeraseme-cli', ['--test', 'plan_execute_live_process'], [], 2),
        ('headers', 'symeraseme-cli', ['--test', 'mcp_http_headers_process'],
         ['native_http_complete_headers_and_bodies_match_go', '--', '--exact'], 1),
        ('smtp', 'symeraseme-core', ['--test', 'campaign_smtp', '--test', 'smtp_transport'], [], 3),
        ('agent-error', 'symeraseme-cli', ['--test', 'mcp_agent_error_json_windows'], [], 1),
        ('native-triage', 'symeraseme-cli', ['--test', 'triage_native_process'], [], 2),
    ]
    if sys.platform != 'win32':
        commands.append(('wire', 'symeraseme-cli', ['--test', 'mcp_http_process'],
                         ['go_oracle_http_wire_transcripts_match', '--', '--exact'], 1))
    for name, package, targets, selection, expected in commands:
        command = ['cargo', '+1.98.0', 'test', '-p', package, *targets,
                   '--locked', '--offline', *selection]
        # Original subprocess capture limits and deadlines are unchanged.
        log = output / ('native-reference-' + name + '.log')
        with log.open('x', encoding='utf-8') as stream:
            result = subprocess.run(command, cwd=source, env=environment,
                                    stdout=stream, stderr=subprocess.STDOUT, timeout=600)
        assert log.stat().st_size <= 4 * 1024 * 1024
        text = log.read_text(encoding='utf-8')
        print(text, end='', flush=True)
        assert result.returncode == 0, 'original live comparator failed: ' + name
        totals = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)
        assert totals and sum(int(p) for p, _, _ in totals) == expected
        assert all(f == '0' and i == '0' for _, f, i in totals), 'zero or skipped cases are not capture proof'


if __name__ == '__main__':
    main()
