#!/bin/bash
# Build the self-contained Rust app backend.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESTINATION="${1:?usage: build-app-backends.sh DESTINATION [debug|release]}"
PROFILE="${2:-debug}"
case "$PROFILE" in
    debug) PROFILE_ARGS=() ;;
    release) PROFILE_ARGS=(--release) ;;
    *) echo "Unsupported backend build profile: $PROFILE" >&2; exit 2 ;;
esac

mkdir -p "$DESTINATION"
DESTINATION="$(cd "$DESTINATION" && pwd)"
BUILD_MESSAGES="$(mktemp)"
trap 'rm -f "$BUILD_MESSAGES"' EXIT
cd "$REPO_ROOT"
cargo build --locked -p symeraseme-cli --bin symeraseme-rust \
    "${PROFILE_ARGS[@]}" --message-format=json > "$BUILD_MESSAGES"
RUST_BINARY="$(python3 - "$BUILD_MESSAGES" <<'PY'
import json, pathlib, sys
binaries = [record['executable'] for line in pathlib.Path(sys.argv[1]).read_text().splitlines()
            if (record := json.loads(line)).get('reason') == 'compiler-artifact'
            and record.get('target', {}).get('name') == 'symeraseme-rust'
            and record.get('executable')]
assert len(binaries) == 1, 'Expected exactly one Rust backend executable'
print(binaries[0])
PY
)"
test -x "$RUST_BINARY"
cp "$RUST_BINARY" "$DESTINATION/symeraseme"
chmod 0755 "$DESTINATION/symeraseme"
