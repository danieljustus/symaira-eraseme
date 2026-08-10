#!/bin/bash
set -euo pipefail

# Make sure we are at the repository root
cd "$(dirname "$0")/.."

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
APP_BUNDLE="$STAGE_DIR/SymairaEraseMe.app"

echo "Creating App Bundle structure..."
rm -rf "$STAGE_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

echo "Copying binary..."
cp "$BUILD_DIR/SymairaEraseMe" "$APP_BUNDLE/Contents/MacOS/"

echo "Writing Info.plist..."
cat <<EOF > "$APP_BUNDLE/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>SymairaEraseMe</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.symaira.eraseme.app</string>
    <key>CFBundleDisplayName</key>
    <string>Symaira EraseMe</string>
    <key>CFBundleName</key>
    <string>Symaira EraseMe</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.10.3</string>
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
rm -f "dist/SymairaEraseMe.dmg"
mkdir -p dist
scripts/create-symaira-dmg.sh \
    "$APP_BUNDLE" \
    "dist/SymairaEraseMe.dmg" \
    "Symaira EraseMe"

echo "DMG successfully created: dist/SymairaEraseMe.dmg"
