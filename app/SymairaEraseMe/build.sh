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
swift build "$@"

SWIFT_BIN_PATH="$(swift build --show-bin-path "$@")"
PROJECT_ROOT="$(cd ../.. && pwd)"
GO_VERSION_VALUE="${VERSION:-dev}"
GO_BINARY="$SWIFT_BIN_PATH/symeraseme"
echo "Building the self-contained Go MCP server..."
(
    cd "$PROJECT_ROOT"
    CGO_ENABLED=0 go build -trimpath \
        -ldflags "-s -w -X main.versionValue=$GO_VERSION_VALUE" \
        -o "$GO_BINARY" ./cmd/symeraseme
)

echo "Build successful!"
echo "Run with: $SWIFT_BIN_PATH/SymairaEraseMe"
echo "Or open in Xcode: open Package.swift"
