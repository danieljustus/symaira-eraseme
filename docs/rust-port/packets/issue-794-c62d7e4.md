# Issue 794 context packet

- **Task:** harden macOS app/DMG signing, notarization, and published-asset verification
- **Base SHA:** `c62d7e40a74f49469cec0560d238b73935221047`
- **Allowed files:** release workflow, DMG packaging script, focused workflow/package tests, changelog
- **Relevant rows:** `REL-007`–`REL-009`
- **Acceptance:** fail-closed credentials; sign app internals; notarize/staple app before DMG; sign/notarize/staple DMG; upload then redownload exact bytes and verify before checksum/note mutation.
- **Commands:** shell syntax, actionlint, native workflow contract tests, package mock tests, full Go gates.
- **Remaining external gate:** a real release run must produce Accepted/stapled/published evidence before `REL-008/009` become PASS and issue #794 closes.
