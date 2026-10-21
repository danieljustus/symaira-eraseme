#!/usr/bin/env bash
# Decide whether the native store switchback must run for one pull request.
#
# Fail-closed: the switchback is skipped only when EVERY changed path is a
# confirmed release-candidate-only or unrelated input. Any other path, an empty
# diff, a missing commit or any git error yields run=true (the switchback runs).
# Writes run=true|false to $GITHUB_OUTPUT. Requires BASE_SHA, HEAD_SHA,
# GITHUB_OUTPUT and RUNNER_TEMP.
set -euo pipefail

: "${BASE_SHA:?pull_request base SHA is required}"
: "${HEAD_SHA:?pull_request head SHA is required}"
: "${GITHUB_OUTPUT:?GitHub output file is required}"
: "${RUNNER_TEMP:?runner temp directory is required}"

for sha in "$BASE_SHA" "$HEAD_SHA"; do
  if [[ ! "$sha" =~ ^[0-9a-f]{40}$ ]]; then
    printf 'Invalid pull_request commit SHA: %s\n' "$sha" >&2
    exit 1
  fi
  if ! git cat-file -e "${sha}^{commit}"; then
    printf 'Missing pull_request commit object: %s\n' "$sha" >&2
    exit 1
  fi
done

changed_paths="$(mktemp "$RUNNER_TEMP/switchback-inputs.XXXXXX")"
trap 'rm -f "$changed_paths"' EXIT
if ! git diff --no-renames --name-only -z "$BASE_SHA" "$HEAD_SHA" > "$changed_paths"; then
  printf 'Could not classify pull_request switchback inputs.\n' >&2
  exit 1
fi

run_switchback=true
if [[ -s "$changed_paths" ]]; then
  run_switchback=false
  while IFS= read -r -d '' path; do
    case "$path" in
      # Inputs that re-run the release-candidate build only (rust-prerelease.yml
      # trigger set) and cannot change the switchback comparison.
      README.md|LICENSE|Makefile|regdata.go|go.mod|go.sum|cmd/*|internal/*|registry/*|\
      scripts/stage_rust_release_archives.py|scripts/rust_artifact_sbom.py|\
      scripts/verify_rust_artifact_sbom.py|scripts/verify-release-source.sh|\
      scripts/verify_windows_dll_dependencies.py|tests/verify_release_archives.py|\
      tests/test_rust_artifact_sbom.py|tests/test_rust_release_workflow.py|\
      tests/test_immutable_release_publication.py|tests/test_homebrew_release_workflow.py|\
      tests/test_make_rust_release_contract.py)
        ;;
      # Other workflows do not feed the switchback. The gate's own workflows and
      # this classifier are matched first and therefore always run it.
      .github/workflows/plain-store-switchback.yml|.github/workflows/release.yml|\
      .github/workflows/rust-prerelease.yml|scripts/classify-switchback-inputs.sh)
        run_switchback=true
        break
        ;;
      .github/workflows/*.yml)
        ;;
      *)
        run_switchback=true
        break
        ;;
    esac
  done < "$changed_paths"
fi

printf 'run=%s\n' "$run_switchback" >> "$GITHUB_OUTPUT"
printf 'switchback inputs changed: %s\n' "$run_switchback"
