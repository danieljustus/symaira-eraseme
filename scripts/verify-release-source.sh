#!/usr/bin/env bash
# Pin build/provenance inputs; protected release tags close the remote check/write race.
set -euo pipefail

test "$(git rev-parse HEAD)" = "${GITHUB_SHA:?}"
test "${GITHUB_REF:?}" = "refs/tags/${RELEASE_TAG:?}"
if [ "${GITHUB_REF_PROTECTED:-false}" != true ]; then
  printf '%s\n' '::error::Release tags must be protected against updates and deletion before publication.' >&2
  exit 1
fi
# Fetch only this remote ref. Do not checkout it or overwrite any local tag.
git fetch --no-tags origin "$GITHUB_REF"
test "$(git rev-parse 'FETCH_HEAD^{commit}')" = "$GITHUB_SHA"
