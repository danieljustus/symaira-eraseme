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

echo "Building SymairaDashboard in Release mode..."
cd app/SymairaDashboard
swift build -c release
cd ../..

BUILD_DIR="app/SymairaDashboard/.build/release"
STAGE_DIR="app/SymairaDashboard/.build/dmg-stage"
APP_BUNDLE="$STAGE_DIR/SymairaEraseMe.app"

echo "Creating App Bundle structure..."
rm -rf "$STAGE_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

echo "Copying binary..."
cp "$BUILD_DIR/SymairaDashboard" "$APP_BUNDLE/Contents/MacOS/"

echo "Writing Info.plist..."
cat <<EOF > "$APP_BUNDLE/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>SymairaDashboard</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.symaira.eraseme.app</string>
    <key>CFBundleName</key>
    <string>Symaira EraseMe</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

SRC_ICON="assets/apple-touch-icon.png"
if [ -f "$SRC_ICON" ]; then
    echo "Generating AppIcon.icns..."
    ICONSET_DIR="app/SymairaDashboard/.build/AppIcon.iconset"
    rm -rf "$ICONSET_DIR"
    mkdir -p "$ICONSET_DIR"
    
    sips -z 16 16     "$SRC_ICON" --out "$ICONSET_DIR/icon_16x16.png" > /dev/null 2>&1
    sips -z 32 32     "$SRC_ICON" --out "$ICONSET_DIR/icon_16x16@2x.png" > /dev/null 2>&1
    sips -z 32 32     "$SRC_ICON" --out "$ICONSET_DIR/icon_32x32.png" > /dev/null 2>&1
    sips -z 64 64     "$SRC_ICON" --out "$ICONSET_DIR/icon_32x32@2x.png" > /dev/null 2>&1
    sips -z 128 128   "$SRC_ICON" --out "$ICONSET_DIR/icon_128x128.png" > /dev/null 2>&1
    sips -z 256 256   "$SRC_ICON" --out "$ICONSET_DIR/icon_128x128@2x.png" > /dev/null 2>&1
    sips -z 256 256   "$SRC_ICON" --out "$ICONSET_DIR/icon_256x256.png" > /dev/null 2>&1
    sips -z 512 512   "$SRC_ICON" --out "$ICONSET_DIR/icon_256x256@2x.png" > /dev/null 2>&1
    sips -z 512 512   "$SRC_ICON" --out "$ICONSET_DIR/icon_512x512.png" > /dev/null 2>&1
    sips -z 1024 1024 "$SRC_ICON" --out "$ICONSET_DIR/icon_512x512@2x.png" > /dev/null 2>&1
    
    iconutil -c icns "$ICONSET_DIR" -o "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
    rm -rf "$ICONSET_DIR"
else
    echo "Warning: assets/apple-touch-icon.png not found. App will build without icon."
fi

echo "Creating Applications symlink..."
ln -s /Applications "$STAGE_DIR/Applications"

echo "Creating DMG..."
rm -f "dist/SymairaEraseMe.dmg"
mkdir -p dist
hdiutil create -volname "Symaira EraseMe" -srcfolder "$STAGE_DIR" -ov -format UDZO "dist/SymairaEraseMe.dmg"

echo "DMG successfully created: dist/SymairaEraseMe.dmg"
