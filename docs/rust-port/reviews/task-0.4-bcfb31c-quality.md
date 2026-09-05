# Task 0.4 quality/security review — revision 1

- **reviewed_sha:** `bcfb31cc592a57918cb42e9a1e8dd1ed6db60f35`
- **base_sha:** `bf53346eec234929bedf0314b99e3da85dbb991b`
- **reviewer_lane:** Hermes native reviewer (`gpt-5.6-luna`)
- **verdict:** REQUEST_CHANGES
- **commands:** exact diff/source inspection; decoded-base64 secret scan; fixture size/hash audit

## Important findings

1. Existing fixture directories are removed before a new generation succeeds; generate into a staging tree and swap with rollback.
2. The allow-remote probe briefly binds `0.0.0.0`; prove policy acceptance without a LAN-reachable listener.
3. The fixed `/tmp` runtime root remains raceable despite marker/lock guards; use a unique owned root and normalize only its exact path.
4. The Go build/test step inherits proxy/toolchain state; require Go 1.26.6 and offline module resolution with a sanitized build environment.
5. Bound subprocess output before reading it and guarantee child cleanup on every server/token failure path.
6. Preserve normalized report evidence instead of discarding the entire generated artifact; make consent output determinism explicit.

## Minor findings

- Add a schema identifier to `surface.json`.
- Prefer structural or occurrence-asserted narrow normalization for variable fields.
