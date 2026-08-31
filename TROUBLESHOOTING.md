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

## Migrating a Python-era installation to the Go CLI

The Go port exposes `symeraseme migrate` as a bounded, explicit migration. It
requires separate `--source` and `--destination` directories; use
`--source-config` and `--destination-config` when the Python defaults keep
configuration/profile files under a separate `~/.config/symeraseme` directory.
This prevents an
accidental in-place rewrite while the plan is being reviewed. Start with:

```bash
symeraseme migrate --source /path/to/python-state \
  --destination /path/to/go-state --platform launchd --dry-run --json
```

The detector reports the Python event database (`symeraseme.db`), TOML config,
legacy encrypted profile (`identity.enc`), and Python-generated scheduler
artifacts. If a native launchd or systemd unit is present under `--home`, it is
reported as well. The source is retained; the destination receives the Go
profile name (`identity.encrypted`) and generated Go scheduler content.

### Backup and rollback

A complete recursive copy of the selected source directory is made in
`<destination>.migration-backup` (or `--backup`) before any destination write.
Native scheduler files outside the source directory are copied into the backup
as external artifacts. The backup contains a `.complete.json` marker; an
incomplete or pre-existing unmarked backup is never overwritten.

Migration writes `.migration-state.json` after the backup and after each item.
If the process is interrupted, run the same command again with the same source,
destination, and backup paths. Completed items are skipped and remaining items
resume. To roll back, stop the Go scheduler if it was activated, remove or
move the destination, and restore the backup's `source/` tree and any files in
`external/` to their original locations. The migration command itself never
deletes the Python source or activates native scheduler units.

### Secrets and limitations

Python credentials are normally in the OS keyring, not in the source tree. The
safe default only reports keyring-backed metadata and never reads or copies
secret values. A caller embedding the migration package may inject a
`SecretStore` implementation and opt into copying through `CopySecrets`; the
CLI does not provide a keychain-copy implementation. Reconfigure credentials
manually unless an independently reviewed SecretStore is supplied.

The migration is intentionally not a live-install end-to-end test: it does not
run `launchctl`, `systemctl`, or `crontab`, and it does not verify provider
credentials. Use `--dry-run` first, keep the backup until the Go installation
has been exercised, and treat scheduler activation as a separate, operator-
controlled step.
