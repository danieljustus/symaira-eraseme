#!/usr/bin/env bash
# Runnable proof for scripts/classify-switchback-inputs.sh. Builds a throwaway
# git history per case and checks the run= value the classifier writes.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
classifier="$root/scripts/classify-switchback-inputs.sh"
work="$(mktemp -d "${TMPDIR:-/tmp}/classify-switchback.XXXXXX")"
trap 'rm -rf "$work"' EXIT

failures=0

# case_run <name> <expected> <paths...>: head commit touches <paths> on top of base.
case_run() {
  local name="$1" expected="$2"
  shift 2
  local repo="$work/$name"
  mkdir -p "$repo" "$work/$name-runner"
  git -C "$repo" init -q
  git -C "$repo" config user.email test@example.invalid
  git -C "$repo" config user.name test
  mkdir -p "$repo/crates" "$repo/scripts" "$repo/.github/workflows" "$repo/docs"
  printf 'base\n' > "$repo/crates/lib.rs"
  printf 'base\n' > "$repo/README.md"
  printf 'base\n' > "$repo/scripts/stage_rust_release_archives.py"
  printf 'base\n' > "$repo/.github/workflows/ci.yml"
  printf 'base\n' > "$repo/.github/workflows/plain-store-switchback.yml"
  git -C "$repo" add -A
  git -C "$repo" commit -q -m base
  local base head
  base="$(git -C "$repo" rev-parse HEAD)"
  local p
  for p in "$@"; do
    mkdir -p "$repo/$(dirname "$p")"
    printf 'change %s\n' "$p" >> "$repo/$p"
  done
  git -C "$repo" add -A
  git -C "$repo" commit -q --allow-empty -m head
  head="$(git -C "$repo" rev-parse HEAD)"

  local output="$work/$name-output"
  : > "$output"
  local got status=0
  got="$(cd "$repo" && BASE_SHA="$base" HEAD_SHA="$head" GITHUB_OUTPUT="$output" RUNNER_TEMP="$work/$name-runner" bash "$classifier" 2>&1)" || status=$?
  if [[ "$status" -ne 0 ]]; then
    got="error(exit $status)"
  else
    got="$(cat "$output")"
  fi
  if [[ "$got" == "run=$expected" ]]; then
    printf 'PASS %-38s %s\n' "$name" "$got"
  else
    printf 'FAIL %-38s expected run=%s got: %s\n' "$name" "$expected" "$got" >&2
    failures=$((failures + 1))
  fi
}

# Release-candidate-only and unrelated inputs skip the switchback.
case_run readme-only false README.md
case_run stage-script-only false scripts/stage_rust_release_archives.py
case_run other-workflow-only false .github/workflows/ci.yml
case_run readme-and-other-workflow false README.md .github/workflows/ci.yml

# Switchback-relevant or unknown inputs run it (fail-closed).
case_run crates-change true crates/lib.rs
case_run switchback-workflow true .github/workflows/plain-store-switchback.yml
case_run unknown-new-file true docs/new-note.md
case_run mixed-readme-and-crate true README.md crates/lib.rs
case_run release-only-plus-unknown true scripts/stage_rust_release_archives.py scripts/unlisted-helper.sh

# Invalid or missing commits fail the selector, which also runs the switchback.
bad_sha_status=0
(
  cd "$work"
  BASE_SHA="not-a-sha" HEAD_SHA="$(printf '%040d' 0)" GITHUB_OUTPUT="$work/bad-output" RUNNER_TEMP="$work" bash "$classifier" >/dev/null 2>&1
) || bad_sha_status=$?
if [[ "$bad_sha_status" -ne 0 && ! -s "$work/bad-output" ]]; then
  echo "PASS invalid-sha-fails-closed          exit=$bad_sha_status, no output written"
else
  echo "FAIL invalid-sha-fails-closed: exit=$bad_sha_status" >&2
  failures=$((failures + 1))
fi

if [[ "$failures" -ne 0 ]]; then
  echo "$failures classifier case(s) failed" >&2
  exit 1
fi
echo "all classifier cases passed"
