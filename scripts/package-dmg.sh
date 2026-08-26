#!/bin/bash
set -euo pipefail

# Make sure we are at the repository root
cd "$(dirname "$0")/.."

# Product name with a space (display convention); technical SPM package and
# target names stay unchanged.
APP_NAME="Symaira EraseMe"

# Version for Info.plist and the DMG filename: prefer the git tag that
# triggered the release workflow (v0.10.6 → 0.10.6), fall back to the
# version in pyproject.toml for local builds.
VERSION="${VERSION:-}"
if [ -z "$VERSION" ]; then
    VERSION="$(git describe --exact-match --tags 2>/dev/null | sed 's/^v//' || true)"
fi
if [ -z "$VERSION" ]; then
    VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' pyproject.toml | head -1)"
fi
if [ -z "$VERSION" ]; then
    echo "Error: could not determine version (no tag, no pyproject.toml version)." >&2
    exit 1
fi

# Detect Xcode or Xcode-beta for SwiftUI macro support
if [ -d "/Applications/Xcode.app/Contents/Developer" ]; then
    export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
elif [ -d "/Applications/Xcode-beta.app/Contents/Developer" ]; then
    export DEVELOPER_DIR="/Applications/Xcode-beta.app/Contents/Developer"
else
    echo "Warning: No Xcode found. SwiftUI macros may not resolve."
fi

echo "Building SymairaEraseMe in Release mode..."
cd app/SymairaEraseMe
swift build -c release
cd ../..

BUILD_DIR="app/SymairaEraseMe/.build/release"
STAGE_DIR="app/SymairaEraseMe/.build/dmg-stage"
APP_BUNDLE="$STAGE_DIR/$APP_NAME.app"
DMG_PATH="dist/Symaira-EraseMe-${VERSION}-macos.dmg"

echo "Creating App Bundle structure..."
rm -rf "$STAGE_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

echo "Copying binary..."
cp "$BUILD_DIR/SymairaEraseMe" "$APP_BUNDLE/Contents/MacOS/$APP_NAME"

echo "Writing Info.plist..."
cat <<EOF > "$APP_BUNDLE/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.symaira.eraseme</string>
    <key>CFBundleDisplayName</key>
    <string>Symaira EraseMe</string>
    <key>CFBundleName</key>
    <string>Symaira EraseMe</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

SRC_ICON="assets/branding/AppIcon.icns"
if [ -f "$SRC_ICON" ]; then
    echo "Installing AppIcon.icns..."
    cp "$SRC_ICON" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
else
    echo "Warning: $SRC_ICON not found. App will build without icon."
fi

# --- Code Signing ---
# If CODESIGN_IDENTITY is set, sign the app bundle with Developer ID,
# including Hardened Runtime and timestamp for notarization.
if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    echo "Signing app bundle with identity: $CODESIGN_IDENTITY"
    codesign --deep --force --timestamp --options runtime \
        -s "$CODESIGN_IDENTITY" \
        "$APP_BUNDLE"
    echo "Verifying signature..."
    codesign -dvvv "$APP_BUNDLE" 2>&1 | head -5
else
    echo "CODESIGN_IDENTITY not set. Skipping code signing (ad-hoc only)."
fi

echo "Creating DMG..."
mkdir -p dist
rm -f dist/SymairaEraseMe.dmg   # legacy unversioned name from older releases
rm -f "$DMG_PATH"
scripts/create-symaira-dmg.sh \
    "$APP_BUNDLE" \
    "$DMG_PATH" \
    "Symaira EraseMe"

echo "DMG successfully created: $DMG_PATH"
