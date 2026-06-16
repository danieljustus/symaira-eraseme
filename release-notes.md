## What's changed

### Features
- #354 Add state field to Address schema for US/CCPA web forms — closes #353

### Fixes
- #366 Derive version from package metadata via importlib.metadata — closes #355, #358
- #367 Security and UX improvements from code review — closes #356, #357, #360, #361

### Security
- #356 Consent token dry-run usage hint updated to prefer --consent-file and warn about shell history/ps exposure
- #357 IMAP adapter now resolves passwords internally via resolve_secret() (no plaintext in service layer)
- #361 LLM factory raises LLMProviderError when API key resolution fails

### Refactoring
- #351 Update symvault subprocess calls to use --print flag

### Closed Issues
- #353 Missing state field in Address schema for US/CCPA web forms
- #355 Version string out of sync with pyproject.toml
- #356 Consent token suggestion exposes token in shell history
- #357 IMAP password passed as plaintext parameter through service layer
- #358 Use importlib.metadata for --version in CLI
- #360 Unify --jurisdiction and --law flags to single canonical name
- #361 Log warning when LLM factory catches SecretResolutionError

**Full Changelog**: https://github.com/danieljustus/symaira-eraseme/compare/v0.3.0...v0.4.0
