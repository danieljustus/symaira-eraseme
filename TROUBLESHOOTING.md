# Troubleshooting

## macOS 27 (Tahoe) — `pydantic_core` / `rpds` LINKEDIT crash

### Symptoms

```
ImportError: dlopen(.../pydantic_core/_pydantic_core.cpython-312-darwin.so):
  mis-aligned LINKEDIT string pool, fileOffset=0x00418E4C
```

Same error for `rpds/rpds.cpython-312-darwin.so`.

### Root Cause

macOS 27 beta's stricter `dlopen` rejects Mach-O `.so` files where the LINKEDIT
segment's string pool has a non-page-aligned file offset. All Rust-based Python
extensions (pydantic_core, rpds-py) built from source with PyO3/maturin produce
such binaries. Pre-built wheels from PyPI may or may not have this issue depending
on the build environment used by the package maintainer.

### Workaround (Homebrew install)

The `danieljustus/tap` formula includes a `post_install` block that automatically
fixes this on macOS 27+. Just run:

```bash
brew reinstall danieljustus/tap/symeraseme
```

If the auto-fix doesn't work (e.g., network issues during install), apply manually:

```bash
# 1. Replace pydantic_core .so with pre-built wheel
VENV_SITE="$(brew --prefix symeraseme)/libexec/lib/python3.12/site-packages"
cd /tmp
curl -sL -o pc.whl "https://files.pythonhosted.org/packages/6c/70/2988cb5112b892b7dc13af570ff57d0f383f770fc88bbb644262df1b3017/pydantic_core-2.47.0-cp312-cp312-macosx_11_0_arm64.whl"
unzip -o pc.whl pydantic_core/_pydantic_core.cpython-312-darwin.so -d "$VENV_SITE/"

# 2. Patch pydantic version check
sed -i '' "s/_COMPATIBLE_PYDANTIC_CORE_VERSION = '2.46.4'/_COMPATIBLE_PYDANTIC_CORE_VERSION = '2.47.0'/" "$VENV_SITE/pydantic/version.py"

# 3. Verify
symeraseme --version
```

### Workaround (source install / pip)

If you installed via `pip install symeraseme` or `uv sync`:

```bash
# Use uv to get a compatible build
uv pip install --force-reinstall pydantic_core==2.47.0
# Or build from source with correct Rust toolchain
pip install --force-reinstall --no-binary pydantic_core pydantic_core==2.47.0
```

For `rpds-py`, the issue is similar. If using uv, it may automatically build a
compatible version. For pip, you may need to wait for an upstream PyO3 fix.

### Tracking

- GitHub Issue: [#410](https://github.com/danieljustus/symaira-eraseme/issues/410)
- Upstream: PyO3/maturin needs to generate properly aligned LINKEDIT for macOS 27
- pydantic PR [#13147](https://github.com/pydantic/pydantic/pull/13147) added
  `-headerpad_max_install_names` but this only fixes `install_name_tool` padding,
  not the string pool alignment issue in PyO3-generated binaries.
