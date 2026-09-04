#!/bin/bash
# tests/test_release_dmg.sh
# Direct mock tests for scripts/package-dmg.sh modes, signing gates, and fail-closed behaviors.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_DIR="$(mktemp -d)"
MOCK_BIN="$TEST_DIR/mock_bin"
MOCK_LOG="$TEST_DIR/invocations.log"

cleanup() {
    rm -rf "$TEST_DIR"
    rm -f "$REPO_ROOT/dist"/Symaira-EraseMe-*-macos.dmg 2>/dev/null || true
    rm -rf "$REPO_ROOT/app/SymairaEraseMe/.build/dmg-stage" 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$MOCK_BIN"

# --- Setup Mock Executables for package-dmg.sh ---

# mock swift
cat <<'MOCK' > "$MOCK_BIN/swift"
#!/bin/bash
echo "swift $*" >> "$MOCK_LOG"

SWIFT_BIN="$TEST_DIR/mock_swift_bin"
mkdir -p "$SWIFT_BIN"
touch "$SWIFT_BIN/SymairaEraseMe"
chmod +x "$SWIFT_BIN/SymairaEraseMe"

case "$*" in
    *--show-bin-path*)
        echo "$SWIFT_BIN"
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
MOCK

# mock go
cat <<'MOCK' > "$MOCK_BIN/go"
#!/bin/bash
echo "go $*" >> "$MOCK_LOG"

OUT=""
while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
        OUT="$2"
        shift 2
    else
        shift
    fi
done

if [ -n "$OUT" ]; then
    mkdir -p "$(dirname "$OUT")"
    touch "$OUT"
    chmod +x "$OUT"
fi
exit 0
MOCK

# mock codesign
cat <<'MOCK' > "$MOCK_BIN/codesign"
#!/bin/bash
echo "codesign $*" >> "$MOCK_LOG"

case "${1:-}" in
    --remove-signature)
        exit 0
        ;;
    -dvvv)
        echo "Executable=$2"
        echo "Identifier=com.symaira.eraseme"
        echo "CodeDirectory flags=0x10000(runtime)"
        echo "Authority=Developer ID Application: Symaira Inc (TEST1234)"
        exit 0
        ;;
    --verify)
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
MOCK

# mock hdiutil
cat <<'MOCK' > "$MOCK_BIN/hdiutil"
#!/bin/bash
echo "hdiutil $*" >> "$MOCK_LOG"

case "${1:-}" in
    create)
        DMG="${@: -1}"
        mkdir -p "$(dirname "$DMG")"
        echo "mock-dmg-content" > "$DMG"
        exit 0
        ;;
    attach)
        printf '/dev/disk99\tGUID_partition_scheme\t\n/dev/disk99s1\tApple_HFS\t/Volumes/mock_volume\n'
        exit 0
        ;;
    detach)
        exit 0
        ;;
    convert)
        OUT=""
        while [ "$#" -gt 0 ]; do
            if [ "$1" = "-o" ]; then
                OUT="$2"
                shift 2
            else
                shift
            fi
        done
        if [ -n "$OUT" ]; then
            mkdir -p "$(dirname "$OUT")"
            echo "mock-udzo-dmg-content" > "$OUT"
        fi
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
MOCK

# mock osascript
cat <<'MOCK' > "$MOCK_BIN/osascript"
#!/bin/bash
echo "osascript $*" >> "$MOCK_LOG"
exit 0
MOCK

chmod +x "$MOCK_BIN"/*
export PATH="$MOCK_BIN:$PATH"
export MOCK_LOG
export TEST_DIR

# --- Test 1: package-dmg.sh without credentials (local non-release test mode) ---
test_local_test_mode_without_credentials() {
    echo "--- Running Test: local packaging test mode without credentials ---"
    rm -f "$MOCK_LOG"
    rm -f "$REPO_ROOT/dist"/Symaira-EraseMe-*-macos.dmg

    local OUT
    OUT="$(VERSION="0.13.0" "$REPO_ROOT/scripts/package-dmg.sh" 2>&1)"
    echo "$OUT" | grep -q "CODESIGN_IDENTITY not set. Skipping code signing (non-release test mode only)."
    echo "$OUT" | grep -q "DMG successfully created"
    test -f "$REPO_ROOT/dist/Symaira-EraseMe-0.13.0-macos.dmg"
    echo "PASS: local packaging test mode succeeded without credentials"
}

# --- Test 2: package-dmg.sh with REQUIRE_SIGNING fails closed without credentials ---
test_require_signing_fails_closed() {
    echo "--- Running Test: package-dmg.sh fails closed with REQUIRE_SIGNING=true ---"
    rm -f "$MOCK_LOG"
    local EXIT_CODE=0
    VERSION="0.13.0" REQUIRE_SIGNING=true "$REPO_ROOT/scripts/package-dmg.sh" >/dev/null 2>&1 || EXIT_CODE=$?
    if [ "$EXIT_CODE" -eq 0 ]; then
        echo "FAIL: package-dmg.sh should fail closed when REQUIRE_SIGNING=true and CODESIGN_IDENTITY is absent" >&2
        exit 1
    fi
    echo "PASS: package-dmg.sh fails closed when credentials are absent"
}

# --- Test 3: package-dmg.sh --app-only and --dmg-only modes ---
test_package_dmg_modes() {
    echo "--- Running Test: package-dmg.sh --app-only and --dmg-only flags ---"
    rm -f "$MOCK_LOG"
    rm -f "$REPO_ROOT/dist"/Symaira-EraseMe-*-macos.dmg

    # 1. Test --app-only with signing identity
    CODESIGN_IDENTITY="Developer ID Application: Symaira Inc (TEST1234)" \
    VERSION="0.13.0" \
    "$REPO_ROOT/scripts/package-dmg.sh" --app-only

    local APP_BUNDLE="$REPO_ROOT/app/SymairaEraseMe/.build/dmg-stage/Symaira EraseMe.app"
    test -d "$APP_BUNDLE"
    test ! -f "$REPO_ROOT/dist/Symaira-EraseMe-0.13.0-macos.dmg"

    # Verify nested Go binary signed before outer app bundle
    grep -q "codesign.*symeraseme" "$MOCK_LOG"
    grep -q "codesign.*Symaira EraseMe.app" "$MOCK_LOG"

    # 2. Test --dmg-only with signing identity
    CODESIGN_IDENTITY="Developer ID Application: Symaira Inc (TEST1234)" \
    VERSION="0.13.0" \
    "$REPO_ROOT/scripts/package-dmg.sh" --dmg-only

    test -f "$REPO_ROOT/dist/Symaira-EraseMe-0.13.0-macos.dmg"
    grep -q "codesign.*Symaira-EraseMe-0.13.0-macos.dmg" "$MOCK_LOG"

    echo "PASS: package-dmg.sh --app-only and --dmg-only succeeded"
}

test_local_test_mode_without_credentials
test_require_signing_fails_closed
test_package_dmg_modes

echo "=== All package-dmg.sh mock tests passed! ==="
