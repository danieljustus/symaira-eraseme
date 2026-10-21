#!/usr/bin/env python3
"""Explicit Git-object preparation for offline frozen-observation tests."""
import argparse
from pathlib import Path
import subprocess

# Deleted feature branches are not included by checkout's fetch-depth: 0.
# Keep the original capture identities; never substitute a squash revision.
REVISIONS = (
    '0bc6b051890340822448fa3846c96f70022b79ad',  # MCP stdio producer
    '13ddc6a43036d6b34706db2b82bbb5a13c40d6de',  # HTTP/SMTP/plan/triage/agent-error producers
    '735ae51ed9c49ca6df88f78c108df8b83f928877',  # Native Windows HTTP producer
    'a3db5a02f9a9aa08d172228caf709c621af3c0bb',  # Native token/consent ACL producer
    '968e1a2dda9c621ba880e7b618dc49daa31970b3',  # Rustls/Yoke native dependency refresh
    'ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33',  # MiniJinja/minor native dependency refresh
    'e61132f85b89580c96d95d098289f7238e810255',  # webpki distrusted-roots native dependency refresh
)
REPOSITORY = 'https://github.com/danieljustus/symaira-eraseme.git'


def prepare(root, fetch=False):
    def missing():
        return [revision for revision in REVISIONS if subprocess.run(
            ['git', '--no-replace-objects', 'cat-file', '-e', revision + '^{commit}'],
            cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            timeout=30,
        ).returncode != 0]

    absent = missing()
    if absent and fetch:
        # Public, immutable objects only. Do not change checkout, refs, index,
        # FETCH_HEAD or shallow boundaries; tests themselves never fetch.
        subprocess.run(
            ['git', '--no-replace-objects', '-c', 'fetch.fsckObjects=true',
             'fetch', '--no-tags', '--no-write-fetch-head', '--no-recurse-submodules',
             REPOSITORY, *absent],
            cwd=root, check=True, timeout=120,
        )
        absent = missing()
    if absent:
        raise RuntimeError('Missing frozen producer objects: ' + ', '.join(absent)
                           + '. Run python3 scripts/prepare-frozen-history.py --fetch first.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fetch', action='store_true',
                        help='download missing pinned objects; without this flag, only check')
    args = parser.parse_args()
    try:
        prepare(Path(__file__).resolve().parent.parent, args.fetch)
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, str(error) + '\n')
    print(f'Frozen producer history available: {len(REVISIONS)} immutable revisions')
