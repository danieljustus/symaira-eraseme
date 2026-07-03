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

if [ $? -eq 0 ]; then
    echo ""
    echo "Build successful!"
    echo "Run with: .build/debug/SymairaEraseMe"
    echo "Or open in Xcode: open Package.swift"
fi
