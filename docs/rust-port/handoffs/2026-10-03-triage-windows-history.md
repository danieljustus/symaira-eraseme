# Windows reply-triage checkout correction

Native run 37125495381 at 8965ffea61a73721b18124375c03a622ac9cc490 passed the newly compiled-agent CLI/MCP triage cases on Windows amd64 and arm64. The subsequent retained 175-case CLI check failed in jobs 111209858235 and 111209858177 before executing its oracle: `git archive 4e582f28` could not find the pinned historical Go source in the default shallow checkout.

The native triage workflow now fetches complete history, as the existing native Rust workflow does. The pinned source revision, exact comparison, native Go execution, test count, and process deadlines remain unchanged. The failed run is retained; a new native run must prove the full gate before acceptance.
