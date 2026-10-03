# Native Windows MCP request staging

Full-workspace SMTP run `37123623125` at `419d60c9` failed on Windows arm64
job `111204464694` in the retained `native_windows_http_matches_checked_out_go`
fixture, after all SMTP transport/campaign controls passed separately on both
Windows architectures. The HTTP parser found no complete response headers.
The fixture sent headers, their terminator and the body in separate TCP writes;
an early auth/Origin rejection can close while the unread body is still arriving.
The older Content-Length guard only checked truncation after a reported reset.

The fixture now stages the complete request before a single bounded write,
matching the existing complete-header comparator's repair. It uses the shared
five-second total bounded reader and requires complete headers and exact
Content-Length for every close outcome. Empty or truncated responses still fail;
there is no retry, relaxed response comparison or increased deadline. Production
HTTP/SMTP behavior is unchanged by this fixture repair. Existing bounded-reader
FIN/reset/truncation/deadline/size controls remain selected in the native MCP gate.
Native candidate evidence remains pending; the original failure is preserved.
