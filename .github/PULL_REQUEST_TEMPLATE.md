## Description

Please include a summary of the change and which issue is fixed.

Fixes # (issue)

## Type of Change

- [ ] Bug fix
- [ ] New feature
- [ ] Documentation update
- [ ] Registry addition (new broker, jurisdiction, etc.)
- [ ] Refactor / chore

## Checklist

- [ ] I have linked the issue this PR addresses (see above).
- [ ] I have run `pytest` and all tests pass.
- [ ] I have run `ruff check .` and `mypy src/symeraseme/` with no errors.
- [ ] I have added tests for any new functionality.
- [ ] I have updated the README or documentation if this change is user-facing.
- [ ] I have checked for sensitive data (API keys, credentials, etc.) in my changes.
- [ ] My commit messages follow the project's style (descriptive, no `WIP`).

## Registry Additions (fill in if applicable)

If this PR adds or modifies broker YAML files, complete this section.

- [ ] YAML validates against `registry/schemas/broker.schema.json` (`symeraseme registry validate`)
- [ ] Broker `id` is unique and matches the filename (e.g. `beenverified-us` → `beenverified.yaml`)
- [ ] Opt-out URL / email address is reachable and verified manually
- [ ] At least one `ack_keywords` entry reflects the actual confirmation text from the broker
- [ ] CAPTCHAs: site_key is the live value from the page DOM (not a placeholder)
- [ ] `disabled: true` is set for any opt-out channel that cannot yet be verified end-to-end
- [ ] No personal data, real API keys, or solver credentials included in the YAML

**Brokers added / modified:**

| File | Broker | Region | Opt-out type | Status |
|------|--------|--------|-------------|--------|
| `registry/brokers/.../xxx.yaml` | Name | US/EU/UK | email/web_form | new/updated |

## Additional Context

Add any other context about the pull request here.
