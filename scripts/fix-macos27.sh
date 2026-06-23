#!/bin/bash
# Fix symeraseme on macOS 27 (Tahoe) — LINKEDIT alignment issue
# See TROUBLESHOOTING.md for details.
set -euo pipefail

if [[ "$(uname)" != "Darwin" ]]; then
  echo "❌ This script is macOS only."
  exit 1
fi

MACOS_VERSION=$(sw_vers -productVersion | cut -d. -f1)
if [[ "$MACOS_VERSION" -lt 27 ]]; then
  echo "✅ macOS $MACOS_VERSION — no fix needed."
  exit 0
fi

echo "🔧 macOS $MACOS_VERSION detected — applying LINKEDIT fix..."

# Find the venv site-packages
if command -v brew &>/dev/null && brew list --formula 2>/dev/null | grep -q symeraseme; then
  SITE_PACKAGES="$(brew --prefix symeraseme)/libexec/lib/python3.12/site-packages"
elif [[ -d ".venv" ]]; then
  SITE_PACKAGES=".venv/lib/python3.12/site-packages"
else
  echo "❌ Cannot find symeraseme installation."
  echo "   Install via: brew install danieljustus/tap/symeraseme"
  exit 1
fi

# Fix pydantic_core
echo "📦 Replacing pydantic_core .so with pre-built wheel..."
PC_SO="$SITE_PACKAGES/pydantic_core/_pydantic_core.cpython-312-darwin.so"
if [[ -f "$PC_SO" ]]; then
  PC_WH_URL="https://files.pythonhosted.org/packages/6c/70/2989cb5112b892b7dc13af570ff57d0f383f770fc88bbb644262df1b3017/pydantic_core-2.47.0-cp312-cp312-macosx_11_0_arm64.whl"
  TMPDIR=$(mktemp -d)
  curl -sL -o "$TMPDIR/pc.whl" "$PC_WH_URL"
  unzip -o "$TMPDIR/pc.whl" "pydantic_core/_pydantic_core.cpython-312-darwin.so" -d "$TMPDIR/"
  cp "$TMPDIR/pydantic_core/_pydantic_core.cpython-312-darwin.so" "$PC_SO"
  rm -rf "$TMPDIR"
  echo "   ✅ pydantic_core .so replaced"
else
  echo "   ⚠️  pydantic_core .so not found at $PC_SO"
fi

# Patch pydantic version check
VERSION_PY="$SITE_PACKAGES/pydantic/version.py"
if [[ -f "$VERSION_PY" ]]; then
  if grep -q "_COMPATIBLE_PYDANTIC_CORE_VERSION = '2.46.4'" "$VERSION_PY"; then
    sed -i '' "s/_COMPATIBLE_PYDANTIC_CORE_VERSION = '2.46.4'/_COMPATIBLE_PYDANTIC_CORE_VERSION = '2.47.0'/" "$VERSION_PY"
    echo "   ✅ pydantic version check patched"
  else
    echo "   ⚠️  pydantic version check already patched or different version"
  fi
fi

# Verify
echo ""
echo "🧪 Verifying..."
if symeraseme --version 2>/dev/null; then
  echo ""
  echo "✅ Fix applied successfully!"
else
  echo ""
  echo "❌ symeraseme still not working. Check TROUBLESHOOTING.md for manual steps."
  exit 1
fi
