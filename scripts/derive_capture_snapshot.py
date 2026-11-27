#!/usr/bin/env python3
"""Derive a frozen Go capture snapshot commit from a product candidate.

The snapshot is built with Git plumbing in a caller-provided scratch worktree;
the caller's checkout and the current branch are never modified. The derived
tree is the candidate tree plus (a) the retired Go producer files of the
predecessor snapshot, byte-identical to 22615ae, (b) the predecessor capture
workflow with only its candidate constant replaced, (c) the predecessor capture
script unchanged, and (d) the single #1330 product-version override. Nothing
else may differ. Creating the local tag is opt-in; pushing and dispatching
capture CI stay manual coordinator steps.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

RETIREMENT = '22615ae67325c581fbe60cb0f2d3d2b2139cf900'
WORKFLOW = '.github/workflows/frozen-http-aux-capture.yml'
CAPTURE_SCRIPT = 'scripts/capture-native-process-references.py'
PROTOCOL = 'crates/symeraseme-cli/src/mcp/protocol.rs'
EXCEPTIONS = frozenset((WORKFLOW, CAPTURE_SCRIPT, PROTOCOL))
PROTECTED = (
    'Cargo.toml',
    'Cargo.lock',
    'fuzz/Cargo.lock',
    'crates/symeraseme-cli/tests/support/native_host_agent.rs',
)
PRODUCT_LITERAL = b'version: env!("CARGO_PKG_VERSION"),'
CAPTURE_LITERAL = b'version: "dev",'
CANDIDATE_CONSTANT = re.compile(rb"candidate = '([0-9a-f]{40})'")
SHA = re.compile(r'^[0-9a-f]{40}$')


class DerivationError(RuntimeError):
    """A derivation or verification invariant failed."""


def git(repo, *args, env=None, input_bytes=None):
    """Run git with an explicit repository path; never rely on the CWD."""
    command = ['git', '-C', str(repo), *args]
    result = subprocess.run(command, input=input_bytes, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, env=env)
    if result.returncode:
        raise DerivationError('git ' + ' '.join(args) + ' failed: '
                              + result.stderr.decode('utf-8', 'replace').strip())
    return result.stdout


def git_result(repo, *args):
    command = ['git', '-C', str(repo), *args]
    return subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def text(repo, *args, env=None):
    return git(repo, *args, env=env).decode('utf-8')


def commit_exists(repo, revision):
    resolved = git_result(repo, 'rev-parse', '--verify', '--end-of-options',
                          revision + '^{commit}')
    if resolved.returncode:
        raise DerivationError('missing commit object: ' + revision)
    resolved = resolved.stdout.decode('ascii').strip()
    if not SHA.match(resolved):
        raise DerivationError('revision resolved outside the expected object format: ' + revision)
    return resolved


def tree_entries(repo, revision, env=None):
    """Return {path: (mode, oid)} for a tree-ish, including nested paths."""
    raw = git(repo, 'ls-tree', '-r', '-z', revision, env=env)
    entries = {}
    for item in raw.split(b'\0'):
        if not item:
            continue
        header, path = item.split(b'\t', 1)
        mode, kind, oid = header.split()
        if kind != b'blob':
            raise DerivationError('unexpected non-blob tree entry: ' + path.decode())
        entries[path.decode('utf-8')] = (mode.decode('ascii'), oid.decode('ascii'))
    return entries


def blob_oid(repo, revision, path, env=None):
    entries = tree_entries(repo, revision, env=env)
    if path not in entries:
        raise DerivationError('missing path ' + path + ' in ' + revision)
    return entries[path][1]


def blob_bytes(repo, oid, env=None):
    return git(repo, 'cat-file', 'blob', oid, env=env)


def blob_at(repo, revision, path, env=None):
    return blob_bytes(repo, blob_oid(repo, revision, path, env=env), env=env)


def hash_blob(repo, data, env=None):
    return git(repo, 'hash-object', '-w', '--stdin', env=env,
               input_bytes=data).decode('ascii').strip()


def is_retired_producer(path):
    if path.endswith(('.go', '.go.in', '.gotmpl')) or path in ('go.mod', 'go.sum'):
        return True
    return (path.startswith(('crates/symeraseme-cli/tests/', 'crates/symeraseme-core/tests/'))
            and path.endswith('.rs'))


def replace_once(data, old, new, label):
    count = data.count(old)
    if count != 1:
        raise DerivationError(label + ': expected exactly one ' + repr(old)
                              + ', found ' + str(count))
    result = data.replace(old, new, 1)
    if result == data:
        raise DerivationError(label + ': replacement made no change')
    return result


def workflow_for(repo, predecessor, old_candidate, candidate, env=None):
    original = blob_at(repo, predecessor, WORKFLOW, env=env)
    if b'#1330' not in original or b'rustup toolchain install 1.98.0 1.98.1' not in original:
        raise DerivationError('predecessor workflow lacks the #1330/#1334 generation '
                              'required for a current capture snapshot')
    old = ('candidate = ' + repr(old_candidate)).encode('ascii')
    new = ('candidate = ' + repr(candidate)).encode('ascii')
    return replace_once(original, old, new, WORKFLOW)


def protocol_for(repo, candidate, env=None):
    original = blob_at(repo, candidate, PROTOCOL, env=env)
    return replace_once(original, PRODUCT_LITERAL, CAPTURE_LITERAL, PROTOCOL)


def predecessor_candidate(repo, predecessor, env=None):
    data = blob_at(repo, predecessor, WORKFLOW, env=env)
    found = CANDIDATE_CONSTANT.findall(data)
    if len(found) != 1:
        raise DerivationError(WORKFLOW + ': expected one candidate constant, found '
                              + str(len(found)))
    return found[0].decode('ascii')


def retired_paths(repo, predecessor, old_candidate, env=None):
    """Paths that the predecessor snapshot restored from the retired tree."""
    predecessor_tree = tree_entries(repo, predecessor, env=env)
    old_tree = tree_entries(repo, old_candidate, env=env)
    retirement_tree = tree_entries(repo, RETIREMENT, env=env)
    paths = set()
    for path, (_, oid) in predecessor_tree.items():
        if path in EXCEPTIONS:
            continue
        if old_tree.get(path, (None, None))[1] == oid:
            continue
        if retirement_tree.get(path, (None, None))[1] != oid:
            raise DerivationError('predecessor producer path is not byte-identical to '
                                  + RETIREMENT + ': ' + path)
        if not is_retired_producer(path):
            raise DerivationError('unexpected retired producer path: ' + path)
        paths.add(path)
    if not paths:
        raise DerivationError('predecessor snapshot has no retired producer files')
    return paths


def validate_scratch(repo, scratch):
    repo_root = Path(text(repo, 'rev-parse', '--show-toplevel').strip()).resolve()
    scratch = Path(scratch).resolve()
    if scratch == repo_root:
        raise DerivationError('scratch worktree must not be the caller checkout')
    scratch_root = Path(text(scratch, 'rev-parse', '--show-toplevel').strip()).resolve()
    if scratch_root != scratch:
        raise DerivationError('scratch path is not a git worktree root: ' + str(scratch))
    common = Path(text(scratch, 'rev-parse', '--git-common-dir').strip())
    repo_common = Path(text(repo, 'rev-parse', '--git-common-dir').strip())
    if not common.is_absolute():
        common = (scratch / common).resolve()
    if not repo_common.is_absolute():
        repo_common = (repo_root / repo_common).resolve()
    if common.resolve() != repo_common.resolve():
        raise DerivationError('scratch worktree belongs to a different repository')
    if git(scratch, 'status', '--porcelain', '--untracked-files=all').strip():
        raise DerivationError('scratch worktree must be clean before derivation')
    return scratch


def derive(repo, scratch, candidate, predecessor):
    """Build and verify an unattached snapshot commit object; return its SHA."""
    commit_exists(repo, candidate)
    commit_exists(repo, predecessor)
    commit_exists(repo, RETIREMENT)
    scratch = validate_scratch(repo, scratch)
    with tempfile.TemporaryDirectory(prefix='.derive-capture-', dir=scratch) as temporary:
        index = Path(temporary) / 'index'
        env = os.environ.copy()
        env['GIT_INDEX_FILE'] = str(index)
        git(scratch, 'read-tree', candidate, env=env)

        old_candidate = predecessor_candidate(repo, predecessor, env=env)
        commit_exists(repo, old_candidate)
        producers = retired_paths(repo, predecessor, old_candidate, env=env)
        retirement_tree = tree_entries(repo, RETIREMENT, env=env)

        for path in sorted(producers):
            mode, oid = retirement_tree[path]
            git(scratch, 'update-index', '--add', '--cacheinfo',
                mode + ',' + oid + ',' + path, env=env)

        workflow_oid = hash_blob(repo,
                                 workflow_for(repo, predecessor, old_candidate,
                                              candidate, env=env), env=env)
        git(scratch, 'update-index', '--add', '--cacheinfo',
            '100644,' + workflow_oid + ',' + WORKFLOW, env=env)

        script_oid = blob_oid(repo, predecessor, CAPTURE_SCRIPT, env=env)
        if blob_oid(repo, RETIREMENT, CAPTURE_SCRIPT, env=env) != script_oid:
            # The capture script deliberately comes from the predecessor snapshot,
            # not from the older retirement tree. Restore its mode from the tag.
            script_mode = tree_entries(repo, predecessor, env=env)[CAPTURE_SCRIPT][0]
        else:
            script_mode = '100644'
        git(scratch, 'update-index', '--add', '--cacheinfo',
            script_mode + ',' + script_oid + ',' + CAPTURE_SCRIPT, env=env)

        protocol_oid = hash_blob(repo, protocol_for(repo, candidate, env=env), env=env)
        git(scratch, 'update-index', '--add', '--cacheinfo',
            '100644,' + protocol_oid + ',' + PROTOCOL, env=env)

        tree = text(scratch, 'write-tree', env=env).strip()
        verify_tree(repo, tree, candidate, predecessor, old_candidate, producers)
        message = 'capture: native dependency refresh for ' + candidate[:8]
        snapshot = text(scratch, 'commit-tree', tree, '-p', candidate,
                        '-m', message).strip()
        return snapshot


def verify_tree(repo, tree, candidate, predecessor, old_candidate, producers):
    result = tree_entries(repo, tree)
    candidate_tree = tree_entries(repo, candidate)
    predecessor_tree = tree_entries(repo, predecessor)
    retirement_tree = tree_entries(repo, RETIREMENT)
    changed = {path for path in set(result) | set(candidate_tree)
               if result.get(path) != candidate_tree.get(path)}
    expected = producers | EXCEPTIONS
    if changed != expected:
        raise DerivationError('tree differs from candidate outside the documented set: '
                              + repr(sorted(changed ^ expected)))
    for path in producers:
        if result[path] != retirement_tree[path]:
            raise DerivationError('retired producer not byte-identical to ' + RETIREMENT
                                  + ': ' + path)
    if result[WORKFLOW][1] != hash_blob(repo, workflow_for(repo, predecessor,
                                                          old_candidate, candidate)):
        raise DerivationError('capture workflow is not the predecessor workflow with '
                              'only the candidate constant changed')
    if result[CAPTURE_SCRIPT][1] != predecessor_tree[CAPTURE_SCRIPT][1]:
        raise DerivationError('capture script differs from the predecessor snapshot')
    if result[PROTOCOL][1] != hash_blob(repo, protocol_for(repo, candidate)):
        raise DerivationError('product override is not the single documented literal')
    for path in PROTECTED:
        if result.get(path) != candidate_tree.get(path):
            raise DerivationError('protected candidate file changed: ' + path)


def commands(repo, candidate, snapshot):
    tag = 'frozen-go-captures/dependency-refresh-' + candidate[:8]
    return [
        'git -C ' + str(repo) + ' tag -a ' + tag + ' -m '
        + repr('native dependency refresh for ' + candidate) + ' ' + snapshot,
        'git -C ' + str(repo) + ' push origin refs/tags/' + tag,
        'gh workflow run frozen-http-aux-capture.yml --repo '
        'danieljustus/symaira-eraseme --ref ' + tag,
    ], tag


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', required=True, help='product candidate commit SHA')
    parser.add_argument('--predecessor-tag', required=True,
                        help='previous frozen-go-captures/dependency-refresh-* tag')
    parser.add_argument('--scratch-worktree', required=True,
                        help='clean, separate git worktree used for derivation')
    parser.add_argument('--repo', default=str(Path(__file__).resolve().parent.parent),
                        help='repository containing the candidate and tag')
    parser.add_argument('--create-tag', action='store_true',
                        help='create the local annotated tag only; never push')
    parser.add_argument('--json', action='store_true', help='emit machine-readable output')
    args = parser.parse_args(argv)

    repo = Path(args.repo).resolve()
    candidate = commit_exists(repo, args.candidate)
    predecessor = commit_exists(repo, args.predecessor_tag)
    snapshot = derive(repo, args.scratch_worktree, candidate, predecessor)
    command_list, tag = commands(repo, candidate, snapshot)
    if args.create_tag:
        message = 'native dependency refresh for ' + candidate
        git(repo, 'tag', '-a', tag, '-m', message, snapshot)
        tagged = text(repo, 'rev-parse', tag + '^{commit}').strip()
        if tagged != snapshot:
            raise DerivationError('created tag does not point at the derived snapshot')

    payload = {
        'candidate': candidate,
        'predecessor': predecessor,
        'predecessor_tag': args.predecessor_tag,
        'snapshot': snapshot,
        'tag': tag,
        'commands': command_list,
        'tag_created_locally': bool(args.create_tag),
    }
    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print('snapshot: ' + snapshot)
        print('tag: ' + tag)
        for command in command_list:
            print(command)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except DerivationError as error:
        print('derive_capture_snapshot: ' + str(error), file=sys.stderr)
        sys.exit(2)
