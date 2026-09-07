#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICON_SOURCE="$ROOT_DIR/assets/branding/AppIcon.icon"
ICNS_SOURCE="$ROOT_DIR/assets/branding/AppIcon.icns"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

XCODE_VERSION="$(xcodebuild -version | sed -n '1p')"
XCODE_MAJOR="${XCODE_VERSION#Xcode }"
XCODE_MAJOR="${XCODE_MAJOR%%.*}"
if ! [[ "$XCODE_MAJOR" =~ ^[0-9]+$ ]] || (( XCODE_MAJOR < 26 )); then
  printf 'Expected Xcode 26 or newer for .icon compilation; got %s\n' "$XCODE_VERSION" >&2
  exit 1
fi

ACTOOL="$(xcrun --find actool)"
APP_PATH="$TMP_DIR/Symaira EraseMe.app"
COMPILED_DIR="$TMP_DIR/compiled"
mkdir -p "$APP_PATH/Contents/Resources" "$COMPILED_DIR"
cp -R "$ICON_SOURCE" "$APP_PATH/Contents/Resources/AppIcon.icon"
cp "$ICNS_SOURCE" "$APP_PATH/Contents/Resources/AppIcon.icns"
"$ACTOOL" \
  --compile "$COMPILED_DIR" \
  --platform macosx \
  --minimum-deployment-target 14.0 \
  --app-icon AppIcon \
  --output-partial-info-plist "$TMP_DIR/partial.plist" \
  "$ICON_SOURCE"
cp "$COMPILED_DIR/Assets.car" "$APP_PATH/Contents/Resources/Assets.car"
cat > "$APP_PATH/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIconName</key><string>AppIcon</string>
<key>CFBundleIconFile</key><string>AppIcon.icns</string>
</dict></plist>
PLIST
REQUIRE_COMPILED_ICON=true "$ROOT_DIR/scripts/verify-app-icon.sh" "$APP_PATH"
printf 'Real .icon compile regression passed with %s\n' "$XCODE_VERSION"
