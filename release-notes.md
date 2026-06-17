## What's changed

### Features
- #406 Migrate hardcoded paths to Config class to respect `SYMERASEME_DATA_DIR` — closes #405
- #387 Address code review findings: security, UX/DX, architecture, performance

### Fixes
- #401 Log warning on secret resolution failure instead of silent fallback
- #396 Replace bare `except Exception` with specific types in event replay
- #371 Read version from `pyproject.toml` instead of hardcoding fallback

### Security
- #402 Bump `cryptography` 48.0.0 → 48.0.1 (fixes vulnerable OpenSSL in wheels)

### Closed Issues
- #405 Hardcoded paths should respect `SYMERASEME_DATA_DIR`

**Full Changelog**: https://github.com/danieljustus/symaira-eraseme/compare/v0.4.0...v0.5.0
