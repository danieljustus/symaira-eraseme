#!/usr/bin/env bash
set -euo pipefail

# Verify that an assembled app carries the approved native icon package and its
# exact native-rendered ICNS fallback. This is intentionally content-based: a
# renamed or stale icon cannot pass merely because the resource paths exist.
APP_PATH="${1:?usage: $0 <app-bundle> }"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ICON_SOURCE="$REPO_ROOT/assets/branding/AppIcon.icon"
ICNS_SOURCE="$REPO_ROOT/assets/branding/AppIcon.icns"
CHECKSUMS="$REPO_ROOT/assets/branding/AppIcon.sha256"
RESOURCES="$APP_PATH/Contents/Resources"
INFO_PLIST="$APP_PATH/Contents/Info.plist"

[[ -d "$APP_PATH" ]] || { printf 'App bundle not found: %s\n' "$APP_PATH" >&2; exit 1; }
for path in "$ICON_SOURCE/icon.json" "$ICON_SOURCE/Assets/S.png" \
           "$ICON_SOURCE/Assets/signet.png" "$ICNS_SOURCE" "$CHECKSUMS" \
           "$INFO_PLIST"; do
  [[ -f "$path" ]] || { printf 'Missing app icon input: %s\n' "$path" >&2; exit 1; }
done
[[ -d "$RESOURCES/AppIcon.icon" ]] || {
  printf 'Missing native icon package in bundle: %s\n' "$RESOURCES/AppIcon.icon" >&2
  exit 1
}
[[ -f "$RESOURCES/AppIcon.icns" ]] || {
  printf 'Missing ICNS fallback in bundle: %s\n' "$RESOURCES/AppIcon.icns" >&2
  exit 1
}
if ACTOOL="$(xcrun --find actool 2>/dev/null)"; then
  [[ -f "$RESOURCES/Assets.car" ]] || {
    printf 'Missing compiled icon catalog in bundle: %s\n' "$RESOURCES/Assets.car" >&2
    exit 1
  }
else
  printf 'App icon guard: actool unavailable; using ICNS fallback only.\n' >&2
fi

python3 -m json.tool "$ICON_SOURCE/icon.json" >/dev/null
while read -r expected path; do
  [[ -n "${expected:-}" ]] || continue
  actual="$(shasum -a 256 "$REPO_ROOT/$path" | cut -d' ' -f1)"
  [[ "$actual" == "$expected" ]] || {
    printf 'Approved icon checksum mismatch: %s\n' "$path" >&2
    exit 1
  }
done < "$CHECKSUMS"

cmp -s "$ICNS_SOURCE" "$RESOURCES/AppIcon.icns" || {
  printf 'Bundle ICNS differs from approved fallback.\n' >&2
  exit 1
}
diff -qr "$ICON_SOURCE" "$RESOURCES/AppIcon.icon" >/dev/null || {
  printf 'Bundle native icon package differs from approved source.\n' >&2
  exit 1
}

ICON_NAME=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconName' "$INFO_PLIST")
ICON_FILE=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$INFO_PLIST")
[[ "$ICON_NAME" == "AppIcon" ]] || { printf 'Unexpected CFBundleIconName: %s\n' "$ICON_NAME" >&2; exit 1; }
[[ "$ICON_FILE" == "AppIcon.icns" ]] || { printf 'Unexpected CFBundleIconFile: %s\n' "$ICON_FILE" >&2; exit 1; }

printf 'App icon guard passed: %s (compiled Assets.car + native AppIcon.icon + AppIcon.icns fallback)\n' "$APP_PATH"
