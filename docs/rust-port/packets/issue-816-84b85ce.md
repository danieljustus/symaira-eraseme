# Issue 816 context packet

- **Task:** freeze and repair identity-profile interoperability
- **Base SHA:** `84b85ce615b9ed8682c18bfffe060cb4ccaf8946`
- **Allowed files:** identity profile/key code and tests, explicit init CLI wiring, generated identity fixtures/provenance, matrix/state docs
- **Relevant row:** `ID-000`
- **Acceptance:** Python fixture decrypt/hash parity; non-destructive dual filename discovery; decrypt-only key lookup; explicit durable key initialization; atomic profile writes; scoped deletion; no real keychain/home data.
- **Commands:** fixture regeneration, identity/full/race/lint/vet/build/coverage gates.
