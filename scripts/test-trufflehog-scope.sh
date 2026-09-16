#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INCLUDE_PATHS="$ROOT_DIR/.trufflehog-include-paths"
EXCLUDE_PATHS="$ROOT_DIR/.trufflehog-exclude-paths"
TRUFFLEHOG_IMAGE="${TRUFFLEHOG_IMAGE:-ghcr.io/trufflesecurity/trufflehog:3.97.4@sha256:562bc231afa9de3d04de44cfe624252b08207de1fc3cebc5e7ed92bed7f279e4}"

if [[ ! "$TRUFFLEHOG_IMAGE" =~ @sha256:[0-9a-f]{64}$ ]]; then
  printf '%s\n' 'TRUFFLEHOG_IMAGE must use an immutable sha256 digest.' >&2
  exit 1
fi

for command in docker git python3; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'Required command not found: %s\n' "$command" >&2
    exit 127
  fi
done

for filter_file in "$INCLUDE_PATHS" "$EXCLUDE_PATHS"; do
  if [[ ! -f "$filter_file" ]]; then
    printf 'Missing TruffleHog filter file: %s\n' "$filter_file" >&2
    exit 1
  fi
done

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/trufflehog-scope.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

FIXTURE_REPO="$TMP_DIR/repo"
mkdir -p "$FIXTURE_REPO/internal/registry" "$TMP_DIR/config"
cp "$INCLUDE_PATHS" "$TMP_DIR/config/include-paths"
cp "$EXCLUDE_PATHS" "$TMP_DIR/config/exclude-paths"

git -C "$FIXTURE_REPO" init -q
git -C "$FIXTURE_REPO" config user.name 'TruffleHog scope regression'
git -C "$FIXTURE_REPO" config user.email 'trufflehog-scope-regression@example.test'
printf '%s\n' 'package registry' > "$FIXTURE_REPO/internal/registry/sync_test.go"
git -C "$FIXTURE_REPO" add internal/registry/sync_test.go
git -C "$FIXTURE_REPO" commit -q -m 'fixture baseline'
HISTORICAL_FIXTURE_COMMIT="$(git -C "$FIXTURE_REPO" rev-parse HEAD)"

# Assemble the detector input only in the disposable fixture. The committed
# script contains no contiguous credential-like URI and no provider credential.
scheme='https://'
username='fixture-user'
value='format-breaking-value'
host='example.test'
path_suffix='/registry.tar.gz'
synthetic_uri="${scheme}${username}:${value}@${host}${path_suffix}"
printf 'package registry\n\nvar syntheticRejectedURL = "%s"\n' "$synthetic_uri" \
  > "$FIXTURE_REPO/internal/registry/sync_test.go"
git -C "$FIXTURE_REPO" add internal/registry/sync_test.go
git -C "$FIXTURE_REPO" commit -q -m 'fixture protected-scope finding'
PROTECTED_COMMIT="$(git -C "$FIXTURE_REPO" rev-parse HEAD)"
printf 'package registry\n\nvar unrelatedSyntheticURL = "%s"\n' "$synthetic_uri" \
  > "$FIXTURE_REPO/internal/registry/other.go"
git -C "$FIXTURE_REPO" add internal/registry/other.go
git -C "$FIXTURE_REPO" commit -q -m 'fixture outside-scope finding'
OUTSIDE_COMMIT="$(git -C "$FIXTURE_REPO" rev-parse HEAD)"

run_scan() {
  local report="$1"
  shift
  docker run --rm \
    -v "$TMP_DIR:/fixture:ro" \
    "$TRUFFLEHOG_IMAGE" \
    git file:///fixture/repo \
    --exclude-detectors Lob \
    --no-verification \
    --results=unverified \
    --fail \
    --fail-on-scan-errors \
    --no-update \
    --json \
    "$@" > "$report" 2> "$TMP_DIR/scanner-errors.log"
}

PROTECTED_REPORT="$TMP_DIR/protected-results.jsonl"
set +e
run_scan "$PROTECTED_REPORT" \
  --since-commit "$HISTORICAL_FIXTURE_COMMIT" \
  --branch HEAD \
  --include-paths /fixture/config/include-paths
PROTECTED_STATUS=$?
set -e

if [[ "$PROTECTED_STATUS" -ne 183 ]]; then
  printf 'Protected-scope scan expected exit 183, got %s\n' "$PROTECTED_STATUS" >&2
  exit 1
fi

PROTECTED_COUNT="$(python3 -c '
import json
import sys

with open(sys.argv[1], encoding="utf-8") as report:
    rows = [json.loads(line) for line in report if line.strip()]
assert len(rows) == 1
row = rows[0]
assert row.get("DetectorName") == "URI"
git_data = row.get("SourceMetadata", {}).get("Data", {}).get("Git", {})
assert git_data.get("file") == "internal/registry/sync_test.go"
assert git_data.get("commit") == sys.argv[2]
print(len(rows))
' "$PROTECTED_REPORT" "$PROTECTED_COMMIT")"
printf 'Protected-scope TruffleHog regression passed: %s expected path/commit finding(s) detected.\n' "$PROTECTED_COUNT"

PRECEDENCE_REPORT="$TMP_DIR/precedence-results.jsonl"
set +e
run_scan "$PRECEDENCE_REPORT" \
  --since-commit "$HISTORICAL_FIXTURE_COMMIT" \
  --branch HEAD \
  --include-paths /fixture/config/include-paths \
  --exclude-paths /fixture/config/exclude-paths
PRECEDENCE_STATUS=$?
set -e

if [[ "$PRECEDENCE_STATUS" -ne 0 ]]; then
  printf 'Include/exclude precedence scan expected exit 0, got %s\n' "$PRECEDENCE_STATUS" >&2
  exit 1
fi
PRECEDENCE_COUNT="$(python3 -c '
import pathlib
import sys

print(sum(1 for line in pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines() if line.strip()))
' "$PRECEDENCE_REPORT")"
if [[ "$PRECEDENCE_COUNT" -ne 0 ]]; then
  printf 'Include/exclude precedence scan unexpectedly found %s result(s)\n' "$PRECEDENCE_COUNT" >&2
  exit 1
fi
printf '%s\n' 'Include/exclude precedence regression passed: exclude wins when both filters match.'

OUTSIDE_REPORT="$TMP_DIR/outside-results.jsonl"
set +e
run_scan "$OUTSIDE_REPORT" \
  --exclude-paths /fixture/config/exclude-paths
OUTSIDE_STATUS=$?
set -e

if [[ "$OUTSIDE_STATUS" -ne 183 ]]; then
  printf 'Outside-scope scan expected exit 183, got %s\n' "$OUTSIDE_STATUS" >&2
  exit 1
fi

OUTSIDE_COUNT="$(python3 -c '
import json
import sys

with open(sys.argv[1], encoding="utf-8") as report:
    rows = [json.loads(line) for line in report if line.strip()]
assert len(rows) == 1
row = rows[0]
assert row.get("DetectorName") == "URI"
git_data = row.get("SourceMetadata", {}).get("Data", {}).get("Git", {})
assert git_data.get("file") == "internal/registry/other.go"
assert git_data.get("commit") == sys.argv[2]
print(len(rows))
' "$OUTSIDE_REPORT" "$OUTSIDE_COMMIT")"
printf 'Path-exclusion regression passed: %s outside-scope path/commit finding(s) detected.\n' "$OUTSIDE_COUNT"

ERROR_REPORT="$TMP_DIR/error-results.jsonl"
set +e
run_scan "$ERROR_REPORT" \
  --branch trufflehog-scope-regression-invalid-ref
ERROR_STATUS=$?
set -e

if [[ "$ERROR_STATUS" -eq 0 ]]; then
  printf '%s\n' 'Scan-error regression expected a non-zero exit, got 0.' >&2
  exit 1
fi
ERROR_COUNT="$(python3 -c '
import pathlib
import sys

print(sum(1 for line in pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines() if line.strip()))
' "$ERROR_REPORT")"
if [[ "$ERROR_COUNT" -ne 0 ]]; then
  printf 'Scan-error regression unexpectedly emitted %s result(s)\n' "$ERROR_COUNT" >&2
  exit 1
fi
printf 'Fail-closed scan-error regression passed: non-zero exit %s with no findings.\n' "$ERROR_STATUS"
