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
REQUIRE_COMPILED_ICON="${REQUIRE_COMPILED_ICON:-false}"
COMPILED_ICON=false
if [[ -f "$RESOURCES/Assets.car" ]]; then
  ASSETUTIL="$(xcrun --find assetutil 2>/dev/null || true)"
  [[ -n "$ASSETUTIL" ]] || {
    printf 'Cannot inspect compiled icon catalog: assetutil unavailable.\n' >&2
    exit 1
  }
  ASSET_INFO="$(mktemp)"
  trap 'rm -f "$ASSET_INFO"' EXIT
  "$ASSETUTIL" --info "$RESOURCES/Assets.car" > "$ASSET_INFO"
  python3 - "$ASSET_INFO" <<'PY'
import json
import sys
from pathlib import Path

entries = json.loads(Path(sys.argv[1]).read_text())
if not isinstance(entries, list):
    entries = [entries]
if not any(item.get("AssetType") == "Icon Image" and item.get("Name") == "AppIcon" for item in entries):
    raise SystemExit("compiled Assets.car contains no AppIcon icon entry")
PY
  COMPILED_ICON=true
elif [[ "$REQUIRE_COMPILED_ICON" == "true" ]]; then
  printf 'Missing compiled AppIcon catalog in release-required mode: %s\n' "$RESOURCES/Assets.car" >&2
  exit 1
else
  printf 'App icon guard: compiled AppIcon unavailable; using approved ICNS fallback only.\n' >&2
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

python3 -c 'import plistlib,sys; p=plistlib.load(open(sys.argv[1],"rb")); assert p.get("CFBundleIconName")=="AppIcon"; assert p.get("CFBundleIconFile")=="AppIcon.icns"' "$INFO_PLIST"

if [[ "$COMPILED_ICON" == "true" ]]; then
  printf 'App icon guard passed: %s (compiled AppIcon entry + native AppIcon.icon + AppIcon.icns fallback)\n' "$APP_PATH"
else
  printf 'App icon guard passed: %s (ICNS fallback only; native AppIcon.icon retained)\n' "$APP_PATH"
fi
