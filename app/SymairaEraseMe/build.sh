#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

# Detect Xcode or Xcode-beta for SwiftUI macro support
if [ -d "/Applications/Xcode.app/Contents/Developer" ]; then
    export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
elif [ -d "/Applications/Xcode-beta.app/Contents/Developer" ]; then
    export DEVELOPER_DIR="/Applications/Xcode-beta.app/Contents/Developer"
else
    echo "Warning: No Xcode found. SwiftUI macros may not resolve."
    echo "Install Xcode or Xcode-beta from the Mac App Store."
fi

echo "Building SymairaEraseMe..."
# ponytail: keep the real linked SDK for native chrome while supporting macOS 14.
# Return to Swift Build when LC_BUILD_VERSION.sdk no longer equals deployment target.
swift build --build-system native "$@"

SWIFT_BIN_PATH="$(swift build --build-system native --show-bin-path "$@")"
PROJECT_ROOT="$(cd ../.. && pwd)"
if [ -n "${SYMERASEME_RUST_TEST_BINARY:-}" ]; then
    test -x "$SYMERASEME_RUST_TEST_BINARY"
    cp "$SYMERASEME_RUST_TEST_BINARY" "$SWIFT_BIN_PATH/symeraseme"
else
    PROFILE=debug
    case " $* " in
        *" -c release "*|*" --configuration release "*) PROFILE=release ;;
    esac
    "$PROJECT_ROOT/scripts/build-app-backends.sh" "$SWIFT_BIN_PATH" "$PROFILE"
fi

echo "Build successful!"
echo "Run with: $SWIFT_BIN_PATH/SymairaEraseMe"
echo "Or open in Xcode: open Package.swift"
