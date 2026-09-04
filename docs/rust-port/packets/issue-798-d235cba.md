# Issue 798 context packet

- **Task:** correct Python/Go Fernet interoperability and encrypted-store durability
- **Integration base SHA:** `d235cbaab86ecb329e93c669f414212b7be57c4e`
- **Allowed files:** event-store crypto/cleanup code, fixtures/generator, focused docs and tests
- **Deferred:** Rust implementation remains Phase 4 issue #806; production DB directory wiring remains #796
- **Acceptance commands:** normal and race tests, explicit archived-Python integration tests, Windows compile, formatting/lint/vet/build/coverage
- **Timeout budget:** 30 minutes implementation, 12 minutes per review
- **Resume:** preserve all uncommitted work, read current diff and latest review artifact before retrying.
