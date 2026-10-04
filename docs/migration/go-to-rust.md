# Go → Rust migration ledger

Single resumption entrypoint. Detailed per-slice write-ups live in
`docs/rust-port/handoffs/`; this file is the state, not the narrative.

- Integrated status (2026-09-24): the proof branch landed on GitHub `main` as `3f133e75`. Its exact-head Go CI, Rust CI (including the native OS matrix), general CI and CodeQL completed successfully. No release, cutover, Go removal or paid provider is authorized. The older slice notes below are historical; a green integrated CI run does not prove a retained older Go rollback binary can read schema v2 (see #1035).
- Native shadow archive gate (2026-09-27): migration PR #1066 at `79ef6a85` passed all eight checks: six native Rust release builds, same-run archive/checksum validation, and plain-store switchback. The archives are unsigned one-day workflow artifacts. Production release routing remains Go; this result does not establish signed publication or user-data cutover. The consolidated main-based PR head needs its own exact-head CI.
- Toolchain: go1.27.1, rustc 1.98.0 (oracle capture pinned at go1.26.6, commit `4e582f28`)
- Crates: `symeraseme-core`, `symeraseme-engine`, `symeraseme-cli`, `rust-tests/parity`

## Exact mutation-anchor maintenance for MCP ping (2026-10-04, #1126)

The standard ping addition shifts the existing protocol parser functions by
twelve lines. The four equivalent-mutant exclusions in `.cargo/mutants.toml`
now follow those exact new line/column locations (844, 893, 595, 603).
Each three-line context is byte-identical to the old accepted main source;
the excluded operations and function names are unchanged. The stream anchor
at 91 stays unchanged. No wider exclusion or timeout increase is introduced.
Actual pinned cargo-mutants execution remains pending; this metadata repair
alone does not establish hardening task 8.5 acceptance or complete #1126.

## Current task ownership (2026-10-02)

The native triage fixture preparation also passes both original Linux amd64
parents at clean `9a7d6b526d16d51735e6dc18935e0b2c1fab786d`: all sixteen CLI cases,
all eight MCP raw-frame/saved-effect cases and ten positive CLI invocation
controls. Go uses its original real CLI and Go native child; Rust uses its
real CLI and a separately compiled native Rust synthetic child. Complete
status/stdout/stderr, the existing provider-list-only ordering fold, saved
reply/ordered events and invocation counts remain checked. Strict CLI
all-target Clippy passes. The Rust fixture implements no Go CLI or MCP oracle.
Both parents still require actual Go: measured default references and current
acceptance on five other hosts remain pending.

The native malformed-agent comparator additionally consumes an actual complete
Linux amd64 Go helper record from clean producer
`020bd691459c1107cef5e7ac639eaba58fdf98fb`. The 32024-byte record has SHA-256
`f84c10a3d412b9257a709cba52370af5042116075508247e3283f26a7932214c`;
172 Go inputs and five immutable/current input generators independently verify,
including the measured Git newline policy. Complete Go oracle and helper
stdout/stderr/status remain unchanged. Both measured Go output-limit rejections
are retained. A real native Rust child fixture, compiled with Rust 1.98.0,
produces the same bad UTF-8 bytes and exit 23 and executes both live output-limit
controls. It implements no MCP or Go oracle. The original MCP JSON/frame and
byte-mutation assertions remain; native Windows additionally runs the real Rust
MCP process with that native Rust fixture. Unrecorded hosts and explicit
live/capture modes still execute actual Go.

At clean `5fbb447bd36e31615f0b1520e33e5033ca27fb8c`, the exact dedicated
SDK-free workflow passes **377 tests, zero failures**. The original native-agent
parent now executes with Go absent on Linux amd64; all twenty-one frozen native
records plus the published-release manifest match immutable Git blobs and an
actual autocrlf checkout. The delegated consent parent still executes all
sixteen private cases. Actual captures/current acceptance on the five other
hosts, native triage and six HTTP auxiliary families remain required. This is
scoped preparation; issue #1131 and complete-workspace independence stay open.
The same original native-helper parent passes with private actual current Go;
strict CLI all-target Clippy passes. Only the bounded measured synthetic
observation JSON is uploaded, with no profile, database or cache contents.

The rollback preparation at clean
`486165456bf328a45f09a6529e507953f92f7bfd` passes the exact dedicated
SDK-free workflow: **376 tests, zero failures**, including the original explicit
fallback test with the actual published `v0.12.1` sibling. The SDK stays absent;
this published runtime is retained only for the explicit rollback test until
removal, outside PATH. All original CLI streams/statuses, native/fallback MCP
initialize bytes and missing/invalid-backend controls execute. Three actual
compiled negative runs reject a same-size changed binary, symlink and foreign
native target. All twenty frozen native files plus the actual release manifest
match immutable Git blobs and a real autocrlf checkout. The delegated consent
parent executes all sixteen private cases. The six-native verification script
now also requires this original fallback test with zero skips; five other native
hosts remain pending for this new mode.

All six actual published archive consumers and 24 corruption/target/symlink
controls also pass without a Go SDK at clean `d0443ebb80acf0bbcfaf0fd5e9b307218c532f3f`.
Their bounded metadata independently matches actual SDK output. Optional
SDK-free switchback/backup modes preserve all six/eight original real cases;
new native execution remains pending. A private real Linux amd64 probe confirms
published Go reads owned schema v1 but refuses schema v2. The candidate-built
switchback lane remains intact and cannot prove compatibility of this older
published binary; backup restore loses post-backup Rust writes. Published
preservation/cutover acceptance and issue #1131 stay open. Current source
`6ca3072` capture run `37176774792` passes complete Rust on both Linux, both
Windows and macOS arm64. All five current service archives and ten complete
plan records are independently verified; macOS amd64 remains queued. That
manual capture is preserved before another dispatch.

The original real-network SMTP comparators now also replay measured native
Linux amd64 Go processes from clean producer
`0b57be7f59923fd69eca54b7807743ffbaf6de2d`. The complete campaign record
preserves stdout, stderr, exit status, the original SMTP transaction with its
existing Date-only fold, and complete result/events/projected-plan output.
The transport record preserves all nine original inputs, raw process streams
and complete original client transactions: plain, OAuth2, rejected/challenged
auth, credential echo, missing STARTTLS, rejected DATA/greeting and HELO
fallback. The empty rejected-greeting transaction is the measured result.
Every Rust socket, synthetic TLS-chain control, credential-redaction check,
exact wire comparison and saved-effect assertion still executes. Recorded
Go input ports remain unchanged; Rust connects to its own original native
listener. Neither a Go child nor a Go server is simulated.

Both records independently verify 172 immutable/current Go inputs and four
immutable/current Rust input generators, native unmodified Go 1.26.6 VCS and
all ten successful Go process statuses/streams. Whole-record SHA-256 pins are
`ab0a2a2a89e06287237238452aafc2d82af3f83885d5a1d8c7a584150a6c1d3e`
(campaign, 35149 bytes) and
`99b28b29c764ed8d38cad725fb146eb2cbf5a47797c9cb1fed6b9ba909cd0a12`
(transport, 42549 bytes). Explicit live/capture modes and unrecorded hosts
continue to run actual Go. Current native capture uploads only these two
bounded synthetic observation records, not profiles or databases.

At clean `4f6ee8b0e48abb9a58fd8af34bf5516cee20c9bb`, the exact default
Go-absent workflow passes **375 tests, zero failures**. All three original
SMTP integration tests execute, including all nine transport cases and the
three private TLS-chain controls. The delegated consent child still runs
all sixteen original cases. All twenty retained native files match immutable
Git blobs and a real autocrlf checkout. The same SMTP comparators pass with
private actual Go; strict core/CLI all-target Clippy passes.

A broader full-workspace Go-absent diagnostic at `114fd2a` is retained as a
failure: twelve actual tests across six targets required Go, including these
two now-prepared SMTP families. The remaining fallback, native helper/triage
and six HTTP auxiliary cases need their own measured evidence. The new SMTP
readers require actual capture and current Rust acceptance on all six native
hosts. Issue #1131 remains open; run `37176774792` at `6ca3072` is preserved
until both queued macOS captures complete before another manual dispatch.

The stricter complete-header HTTP comparator now also replays a measured
Linux amd64 record from clean producer
`17cfa0931b853345bd0b14afda3f77f25d66c656`. Its ten original cases retain the
entire raw response: status line, every header including the original Date,
and complete body bytes. The unchanged parser validates exactly one
canonical Date before applying the original `<DATE>` comparison. All other
header values, duplicate-header controls, status assertions and exact body
comparisons remain intact. The actual 39156-byte record has SHA-256
`09a5a5329c9d37839e22b7a6b0c793b5e5cd566dd292a6ac4027c681993b3bdf`;
171 Go sources and three immutable/current generator pins verify. Original
cleanup kills the servers; exit/stdout/stderr are outside this comparator.
Unrecorded targets and explicit live/capture modes still execute actual Go.

At clean `114fd2a5bf1455f874c17a91fe42010954b725de`, the exact default
Go-absent workflow passes **372 tests, zero failures**, including all five
complete-header tests and both ten-case HTTP process comparators. The sole
consent child runs all sixteen isolated cases under its passing parent.
All eighteen retained native files match immutable Git blobs and an actual
autocrlf checkout. The same ten complete-header cases pass with private
actual Go; strict CLI all-target Clippy passes. Current native HTTP reader
acceptance remains pending, with raw complete-header captures required on
all six hosts and the older wire captures on all four Unix hosts.

The Linux amd64 original HTTP wire comparator now consumes a measured actual
Go record from clean producer `b518d619f297754970ac7b44f86497aafd1072ad`.
All ten unchanged request cases still compare real Rust HTTP status,
Content-Type and complete body bytes. Ephemeral native bearer tokens retain
only the original request-token substitution; other response headers and
process stdout/stderr are outside this original comparator. The retained
38331-byte record SHA-256 is
`afc81fa177dc03ee70c6f26ed74334304671d2ea20e6106f9a629876c16e7d32`.
Its 171 actual Go input pins and three immutable/current Rust input generator
pins were independently read back; native Go 1.26.6 VCS is unmodified and
server exit status is zero. Whole-record corruption, truncation and source
changes are rejected before replay. Explicit live/capture modes and the
three unrecorded Unix hosts continue to execute actual Go.

At clean `a2a7219d16179878b5d4094c0565daf212a03822`, the exact default
Go-absent workflow passes **367 tests, zero failures**; the sole delegated
consent child is actually run in all sixteen isolated cases by its parent.
The same original HTTP comparison passes with private actual Go, and strict
CLI all-target Clippy passes. All seventeen retained native files match
immutable Git blobs and an actual autocrlf checkout. The other HTTP cases,
native Windows helpers and fallback transports still require Go.

Current native reader run `37176774792` at `6ca3072` passes complete Rust on
both Linux and both Windows hosts. All four service archives and eight
whole-plan records have been independently read back. Both macOS jobs and
their current archives remain queued; this run is retained before another
manual capture is dispatched. The new HTTP reader awaits its own current
native acceptance, including actual capture on all four Unix hosts. Issue
#1131 remains open.

Actual native run `37172389657` at clean `0972f73` now passes all six
complete Rust suites. Every downloaded Linux/Windows/macOS service archive
has been independently read back: all 1481 immutable source/input pins,
native unmodified Go 1.26.6 VCS, full streams/status and 132 Unix or 111
Windows observations. All twelve separate complete plan-process records
also verify the original stdout/stderr/status and full state/events/manual-
task tuple, 171 Go inputs and three archived input generators. Both Mac
recordings preserve the actual producer bytes; no host result is inferred.

The readers now select measured native records for both complete plan tests
on all six hosts, and the three original Unix process comparisons on all
four Unix hosts. Each Unix manifest is sealed by its measured whole SHA-256;
all six raw streams independently match the original retained fixture bytes.
Changed generators or dependency locks require real recapture. Explicit
live and plan-capture modes still execute actual Go. Current native CI
additionally removes Go from its subprocess PATH and requires both original
plan cases on every host and all three original process cases on Unix.
At clean `c0f598586d5f76529229a6ba9b7c971f29bf7a76`, the exact default
Go-absent workflow passes 366 tests with zero failures. Its sole top-level
consent child is executed in all sixteen private cases by its passing parent.
The same three Unix comparators also pass with private actual Go; strict
CLI all-target Clippy passes. All sixteen newly retained plan records/native
manifests match Git blobs and an actual autocrlf checkout. Current native
reader acceptance and complete-workspace Go independence remain pending;
remaining HTTP, native helper and fallback transports still execute Go.
Issue #1131 remains open. Main `ed6a74f` is integrated; future Go-source
changes from pending MCP integration require actual recapture.

Linux amd64 now prepares frozen references for both complete consented plan
process tests using the actual unmodified records from `fae7b5d`. Whole-record
SHA-256 pins seal stdout/stderr/status, all 171 Go sources and the original
state/events/manual-task tuple. The archived Rust input generators are
verified against their immutable Git blobs. Real Rust processes, exact byte
comparisons and every existing persisted-effect assertion remain intact;
changed/truncated records and fabricated state are rejected. Explicit capture
always executes actual Go, as do explicit live mode and unrecorded targets.
At clean `d401fc085d7203d305883e84806acfe9e6f4c9a7`, the exact Go-absent
workflow passes 366 tests with zero failures. The sole top-level ignored
consent child is actually executed in all sixteen isolated cases by its
passing parent. Both new whole-process tests pass without Go and with private
actual Go; strict CLI Clippy passes. Both raw process records survive Git
and autocrlf byte-exactly. An initial local run retained a stale encryption
test binary from the other checkout in the shared Cargo target; its embedded
build path identifies that source. Sequential recompilation of the current
checkout resolves the cache issue with no source-byte or acceptance change.
The old failed log and binary identity are retained. Current six-target
acceptance and complete-workspace Go independence remain pending.

Actual run `37170268056` at clean `9e1a0bb` passes four complete native Rust
suites (Linux and Windows on amd64 and arm64). All four downloaded artifacts
independently verify all 1481 immutable source/input pins, native unmodified
Go VCS and complete observations: 132 cases/115 exact members on Unix and
111 cases/103 exact members on Windows. Provider cancellation and the Unix
process fixtures match their original byte-exact corpora. Both Mac jobs
remain queued; their acceptance and the new plan recordings are pending.


The existing two complete consented plan-execution process comparators now
prepare an opt-in actual Go recorder. When explicitly selected, it retains
whole stdout/stderr/status and the original persisted state/events/manual-task
tuple for both private synthetic web-form and senderless-email cases. All
171 Go input/source pins, archived Rust input-generator identity and native
Go 1.26.6/unmodified VCS are included; cross-compilation, dirty sources and
existing output files are rejected. The narrow upload contains exactly two
JSON records, excluding databases, consent tokens, profiles and build caches.
Both original complete Go/Rust process comparisons pass in disposable roots
at clean `fae7b5da416db5e491ed3b4d1944069d3f80d291`. Independent readback
validates both whole output/effect records, all 171 immutable Go inputs, all
three archived input-generator identities and actual native Go VCS. Current
native recordings and frozen-reader acceptance remain required.


The subsequent actual 110-observation run `37168958036` at clean
`212c2821eb154544c5948a72f2f24a0a98bcfc12` passes four complete native Rust
suites (both Linux and both Windows). All four downloaded 98-member archives
independently verify every 1476 immutable input/source pins, native Go
1.26.6/unmodified VCS, complete streams and original acceptance controls.
Both macOS jobs remain queued; this evidence does not establish their result.


Actual clean Linux-amd64 capture `8febd4dfa3498065b464ccde79aaaceafaba44c6`
passes 132 observations. Independent readback validates all 1481 immutable
input/source pins, 115 narrowly selected upload members, native unmodified
Go VCS and all 22 added complete observations. CLI triage, malformed-stderr
and provider-cancellation bytes match their original committed fixtures.
Linux amd64 defaults now replay the retained whole CLI triage/MCP triage/
malformed-stderr outputs with status/streams/provenance checks. Complete
live Rust processes, all sixteen CLI cases, all four MCP cases, state effects
and malformed UTF-8 comparison remain unchanged; explicit live mode and all
other native hosts run the real producer. Corruption controls reject changed/
truncated frames and fabricated source identity before replay. The Go-free
workflow adds the three original complete process tests. At clean
`d8dba96a70439d83b4d2dab9d128ff2148d973aa`, its exact Go-absent execution
passes 364 tests with zero failures. The sole top-level ignored consent
harness child is actually executed in all sixteen isolated cases by its
passing parent. The same three complete process comparisons pass with private
actual Go; strict CLI Clippy passes. All seven retained raw files match Git
blobs and a real autocrlf checkout. Native acceptance of these additions and
complete-workspace Go independence remain pending.


The opt-in #1131 producer additionally prepares actual provider cancellation
(one loopback client/provider observation on every target) and the existing
Unix shell-agent corpora: one complete malformed-stderr MCP frame, sixteen
CLI triage cases and four MCP triage cases. Every new binary retains native
unmodified Go VCS, immutable oracle/input pins and the original finite
compile/runtime/output bounds. The actual cancellation file joins the narrow
artifact allowlist. Unix targets record 132 observations; Windows records
111 applicable observations while its separate native-executable controls
remain live. No zero-case substitute is used. The clean local 132-observation
producer and its independent source/stream readback pass; current native
132/111-observation acceptance remains required.


At earlier clean `6f084b0070dff5664c2bd234a571e651d82f58bd`, the exact
Go-absent workflow passes all 361 selected tests with zero failures. Its
sole top-level ignored harness child is actually executed in all sixteen
isolated consent cases by the passing parent and is never counted as
skipped acceptance. All 25 CLI tests pass without Go; the same thirteen-case
schedule comparison passes with private actual Go; strict CLI Clippy passes.
All 102 CLI binary unit tests and three actual-live MCP comparators retain
their `dd51bb0` proof, and all eight triage/projection tests retain their
Go-absent/actual-live proof at `90c50c0`. Four complete native Rust suites
pass at `85acc513` and all four 98-member/110-observation actual Go captures
are independently verified. Their twelve newly retained raw manifests and
scheduler streams match the artifact bytes in Git and autocrlf checkouts.
Current six-target acceptance of the new readers and complete-workspace
Go independence remain required. Issue #1131 stays open.


The thirteen CLI schedule cases now select complete actual Linux2/Windows2
outputs from native source `85acc513`, with native unmodified VCS and all
1476 immutable input pins. Windows bytes are retained separately from Unix
bytes; original Rust stdout/stderr/status and generated-file hashes remain
compared. Explicit live mode executes the original finite Go producer with
offline modules and unchanged process/capture bounds. Unrecorded macOS
retains its existing Unix fixture pending actual native readback. A new
control checks all four full native frames and rejects corruption, case loss
and fabricated source/target identity. Local/current native acceptance is
pending; the expected selected total becomes 361 tests. The first local
reader run retained 23 passes and two failures because it incorrectly
required empty producer stderr. Actual whole schedule captures retain
the producer's case-summary diagnostics there; the reader now verifies those bytes against
the measured length/SHA-256 instead. All per-case stderr comparisons stay
unchanged, At clean `6f084b0070dff5664c2bd234a571e651d82f58bd`, corrected validation
passes all 25 CLI tests without Go, the same thirteen-case comparison passes
with private actual Go, and strict CLI Clippy passes.


Actual 110-observation run `37167564927` at clean `85acc513` records complete
MCP runtime frames on both Linux and both Windows hosts. All four downloaded
98-member archives independently validate all 1476 immutable inputs, native
Go 1.26.6/unmodified VCS, whole streams, case counts and hostile clock-path
controls. All three MCP families are byte-identical to the retained actual
Linux frames. Defaults now additionally validate the producing native
manifest on these four hosts; macOS and explicit live mode retain actual Go.
Original Linux provenance stays verified too. Existing corruption controls
check truncated frames and fabricated target identities on all four native
captures. At clean `dd51bb0`, all 102 CLI binary unit tests pass without Go, including
all 98 MCP tests; the same three changed comparators pass with private real
Go. Strict CLI Clippy also passes at `6f084b0`; current native reader acceptance
remains pending.


The Go-absent workflow now runs every core/engine/CLI library and binary
unit test, replacing the narrow 98-MCP/two-writer selection. At clean
`90c50c0`, 225 actual unit tests pass without Go and no failures occur. The
existing isolated consent child is marked ignored only at top-level libtest:
its passing parent actually launches and verifies all sixteen child cases
with their separate umask/resource limits. It is never credited as skipped
acceptance. The selected total becomes 360 passing tests (prior 235 less
100 replaced tests plus 225 units). The exact full new Go-absent workflow
passes at clean `2fd1645` with 360 passing tests and zero failures; its sole
top-level ignored harness child is actually executed sixteen times by the
passing parent. It contributes no skipped acceptance.
Native six-target reader acceptance and complete-workspace independence
remain open.


The #1131 triage-service and oversized-projection defaults additionally bind
their complete streams to all six actual native `0d1be282` recordings and
194 immutable source/input hashes. Triage bytes match on every target;
projection uses each target's measured amd64 or arm64 overflow results.
Existing Linux provenance and every Rust service/state comparison remain.
The corruption controls verify all six actual native full frames and reject
truncation. At clean `90c50c01145359ce35f2e81ce78671c2cbb7e827`, all eight
affected triage/projection tests pass with Go absent and in private actual
live-Go mode. Strict core Clippy passes. The selected workflow remains 235
tests; unchanged other-family proof is retained from `0cfba7c`/`48fd77c`.
Current native reader acceptance is pending; no row is promoted.


### Four-target native grant replay and Windows source bytes (2026-10-04, #1131)

Run `37165107890` at clean `344dfaae` passes complete Rust suites on both
Linux and both Windows hosts. All four actual Go artifacts are downloaded
and independently verified: 82 bounded regular members each, 194 immutable
source/input pins, native unmodified Go 1.26.6 VCS, full process streams and
five actual consent records per host. Four raw manifests, 48 grant streams
and twenty whole records are retained unchanged. The grant default selects
those measured targets; macOS and explicit live mode keep real Go. The
existing corruption control checks all four source/target identities and
changed consent payload, filename and host-specific measured mode (Unix
0600, Windows 0666). Actual Rust private file/ACL checks remain unchanged.
At clean `48fd77ca6bde5e0182e4dc1e066af5860ee4ff04`, all 24 CLI tests pass
without Go, the same grant comparator passes with private real Go, and
strict CLI Clippy passes. All 72 raw Git blobs survive autocrlf unchanged.
The selected workflow remains 235 tests; other-family proof is retained
from `0cfba7c`. Native Rust acceptance of these new readers is pending.

Current Windows MCP jobs `111330072961`/`111330072837` exposed Git CRLF
conversion of byte-hashed broker YAML. Explicit LF for all embedded broker
inputs and binary for the pinned database preserve all 1476 recorded input
lengths/hashes under actual `git -c core.autocrlf=true checkout-index` at
`766d41f`. Source checks and corruption controls are unchanged; new native
acceptance is pending. Neither correction closes issue #1131 or promotes
release/cutover rows.

### Linux MCP runtime frozen-reader preparation (2026-10-04, #1131)

The corrected 110-observation producer passes at clean `f0a91ab` on actual
Linux amd64. Independent readback validates all 1476 Go/oracle/broker input
pins, the 98-member upload selection, native unmodified binary provenance,
complete streams and the clock's hostile-path controls. Its six clock,
four auto-confirm and 25 gap responses now supply the Linux amd64 default
comparators; original Rust whole-frame/state/effect checks stay intact.
The other five native targets and explicit live-Go mode retain the actual
producer. A control rejects changed/truncated streams, unknown families
and fabricated source identity. The Go-free workflow adds the complete MCP
unit test group. At clean `0cfba7c1e3587667de602ba3bdb7d3a28670f90a`,
all 98 MCP unit tests and the exact full 235-test default workflow pass with
Go absent (zero failures/ignored tests). Each of the three changed Go-backed
comparisons also passes explicitly selected real Go 1.26.6 in private roots,
including the original hostile project/compiler-temp checks. Strict CLI
Clippy passes. Current native CI remains required; no all-target or complete-
workspace Go independence is claimed.

### Native MCP/Windows schedule capture preparation (2026-10-04, #1131)

The opt-in actual Go producer now records the four remaining runtime-oracle
families: thirteen CLI schedule cases, six fixed-instant MCP clock responses,
four auto-confirm responses with recorded state, and 25 complete MCP gap
responses. Every binary is built from clean native Go 1.26.6 with unmodified
VCS, finite compile/runtime/output limits, private configuration/data, and
whole stdout/stderr/status metadata. Broker YAML inputs and the four oracle
sources join the source inventory. The clock producer runs from a hostile
project and must leave both injected project and inherited data paths absent.
The original 62 observations remain, giving 110 product observations. New
capture at `60c847e` retained a real 1284895-byte MCP gap output, exposing the
shared producer's 1 MiB limit. That family now uses its existing Rust test's
8 MiB complete-frame allowance; all other output limits and every deadline
remain unchanged. Corrected capture validation is pending; no Rust reader
is changed and no new Go-free
or native acceptance is claimed. The current 62-observation run retains its
own source-bound scope and will not be credited for this extension.

### Complete native grant artifact preservation (2026-10-04, #1131)

Review of the opt-in upload allowlist found that the 62-operation producer
wrote five complete synthetic consent observations but the artifact selected
only the grant process streams and their metadata. The upload now includes
only `cli-grant-case-*-consent-*.observations.json` as well. Readback against
the actual clean `b8faa96` Linux capture proves the allowlist grows from
77 to 82 regular members and includes every measured record; private roots
and generated executables stay excluded. This is capture completeness,
not new native Go-free grant acceptance. The preceding `2215b45` native run
cannot supply these omitted record files and will be replaced only after
checking that no active capture loses evidence.

### Six-target campaign and SQLite frozen preparation (2026-10-04, #1131)

The independently read-back six-target `0d1be282` artifacts also contain
byte-identical complete campaign-execution (6341 bytes) and SQLite
(12585 bytes) outputs on every native host. Their default Rust comparisons
now select the actual native manifest, verify all 194 source/input pins and
native unmodified build provenance, and bind each full output's byte count
and SHA before comparing real Rust effects. Campaign planning additionally
checks the native successful tagged Go test and its exact 6628-byte fixture;
SQLite checks all seven native Go corruption/control pass records.
The original Linux fixture integrity, quoted-SQL, archived Python database,
real Rust filesystem/database effects, and explicit bounded live-Go paths
remain enforced. This changes no product observation, contract status or
Go-free test count (137). At clean `2e162369b3b205474ddd52dfac101dd9b08b455e`,
all 53 affected/shared-reader tests pass with Go absent (campaign execution,
plan, SQLite, LLM and whole CLI), and all seventeen campaign/plan/SQLite
tests pass with actual Go 1.26.6 in private HOME/XDG/temp/data roots.
Strict core/CLI Clippy and the graph controls pass. The unchanged 84 other
Go-free tests retain their `a02907c` proof. Current native Rust verification
remains pending. The in-flight native capture run uses the
preceding six-target LLM/review source; it does not accept these new readers.

### Six-target LLM and review frozen preparation (2026-10-04, #1131)

The final macOS arm64 artifact `11288785362` from capture run `37158381207`
was downloaded with its published digest and independently checked against
immutable source `0d1be28288594354941a07d19f834f2553928bc4`:
all 194 source/input files, all 65 archive members, native unmodified Go build
info, seven complete LLM outputs and eight complete review process records.
The whole LLM/review status/stdout/stderr bytes match the existing records.
The later Rust scheduler failure remains retained: launchd own-unit reinstall
reported `Invalid argument (os error 22)` at the shared PID-root fixture.
The branch already replaces shared scheduler roots with separately owned
TempDirs; current six-target Rust acceptance remains required.

Default LLM and review comparisons now require their actual native manifest
on all six targets. Review checks every native argv, status and full stream
hash before using the original measured bytes. An added control verifies all
six recorded hosts and rejects a changed process status. The grant default
remains separately live outside Linux because its native captures are not
complete. Explicit live-Go mode still builds and executes the pinned producer.
The selected Linux Go-free workflow contains 137 tests (24 whole CLI tests),
with all 113 other-family tests passing at `a02907c`. The first new CLI run
exposed a reader field-name error (`input_sha256` instead of the actual nested
`input` record), before process comparisons; the reader now checks both bytes
and SHA from that recorded object. The failure is retained. At clean `32f897aec36ee53ec2c23aed935d9bcb2c62926c`,
all 24 whole CLI tests pass with Go absent; all twelve LLM tests and all three
review tests also pass in an isolated actual live-Go environment. Together
with the unchanged 113 other-family tests at `a02907c`, all 137 selected
Go-free tests are covered; strict core/CLI Clippy passes. Six-target current
Rust acceptance remains pending. No contract row,
phase, issue completion, release, or Go retirement is inferred from captures.

### Reviewed five-target LLM frozen preparation (2026-10-03, #1131)

Five actual Go artifact archives from run `37158381207` at clean source
`0d1be28288594354941a07d19f834f2553928bc4` were downloaded and verified:
Linux amd64/arm64, Windows amd64/arm64 and macOS Intel. Each has exactly 65
bounded regular members; its archive digest, every captured stream and all
194 source/input file hashes match immutable Git contents. All seven LLM
outputs and all eight review CLI outputs are byte-identical to the existing
Linux records on those five actual hosts. Raw native manifests and the archive
readback receipt are retained unchanged under `go-frozen/native-0d`.

The LLM family now selects its actual per-target manifest, verifies native
Go/VCS/OS/architecture and all 194 sources, and compares complete outputs for
the exact observed argument list. Windows and macOS Intel can default to
these frozen producer observations; the real Rust loopback provider, retry
counts, messages, redaction and unchanged fifteen-second active budget remain
executed. Explicit live Go mode remains. Uncaptured Mac arm64 and unknown
architectures keep the actual live producer; no host is inferred from another.
An added control checks all five records and rejects wrong target identity
and missing source inventories, in addition to retained whole-byte and unknown
argument controls. The selected Go-free workflow grows from 135 to 136 tests.
All twelve local LLM tests pass with Go absent, including all five actual
source/target records, the retained provider loopbacks and both fixture
lifecycle controls; strict targeted core Clippy and formatting pass.
Current-source complete-workflow and native Rust acceptance remain pending.

The complete selected workflow passes all 136 tests at clean
`5197905ca2753a616b62336a9d5b679bd622fd96`, zero failures or ignores.
Git's general manifest LF rule initially converted the two captured Windows
manifest files (57,163→55,882 and 57,179→55,898 bytes). An explicit native
capture attribute at `b0c67cb6b6ddf2f09f8d3c23c4feb928d0edc661` preserves
the actual recorded CRLF bytes. All five Git blobs now exactly match the
downloaded artifacts, and a real `core.autocrlf=true` checkout preserves them.
On that clean final source all twelve LLM tests pass both with Go absent and
with the explicitly selected real Go producer in private roots. No output,
source digest, native identity or comparison is rewritten. Current-head
GitHub and full native Rust acceptance remain pending.

This verifies actual Go captures, not the old full native run's success:
its Linux arm64 suite retains the reproduced scheduler root race fixed later
at `aa68478`; macOS Intel's full suite passes. Mac arm64 capture is still
pending, and no new cross-platform Rust Go-independence is claimed before
execution. The review/grant defaults and other runtime families remain
separately scoped; #1131 remains open.

### Complete Linux CLI without Go (2026-10-03, #1131)

At clean `b8faa96fec95ddfffe0776ceea0d83579f639455`, all 23 tests in
the entire `command_surface` binary pass with Go absent from PATH and default
frozen mode. This includes the 175-case recorded CLI corpus, eight populated
status/tick frames, all thirteen scheduler cases with file manifests, profile
round trips, populated plans/manual tasks, eight review processes and six
grant processes with real Rust file effects. Existing assertions and cases
are retained. Windows keeps its actual native Go migration/scheduler gates;
this result does not claim the entire CLI is Go-free on Windows or Mac.

The first local attempt faithfully failed the recorded migration-directory
mode under the workspace's restrictive umask (0700 instead of recorded
0755). Running with the recorded Unix umask 022 passes all 23 tests; no
fixture byte, file mode assertion or product permission rule changes.
The Go-absent workflow now runs this entire binary once, instead of only the
four selected review/grant tests, for 135 total tests in its selected Linux
families. It explicitly uses umask 022 and observes all fixture changes.
Current-source complete-workflow and GitHub validation remain required.
Other runtime-oracle families and six-native acceptance remain open under
#1131; no Go deletion or release/cutover is claimed.

### Consent-grant frozen-oracle preparation (2026-10-03, #1131)

Actual native Go 1.26.6 at clean `aa684787641be9c92a8b0c5a30fa75d804c1c48d`
records six complete grant issue/list/positional-TTL/revoke/revoke-all/empty
process outcomes in a disposable data root. Every raw stream, token-derived
filename, file byte/hash, mode and persisted TTL is retained with 171 source
hashes and the unmodified native VCS identity. Linux defaults use verified
recorded Go expectations and effects; the Rust CLI still genuinely issues,
reads and revokes independent fresh grants. Existing normalization is limited
to random tokens and issued/expires Unix timestamps; all other whole bytes,
filename/hash/mode, count, command and 86,400/60-second TTL checks remain.
No frozen Go files are written into a pretend producer directory. Other
platforms and explicit live mode keep the real Go producer. Both CLI families
share a bounded 120-second offline readonly/VCS build and unchanged ten-second
process/capture bounds. Added controls reject changed bytes, filenames and
modes and retain both persisted and emptied states. Four selected review/grant
tests pass with Go absent locally. The observation pipeline now captures
all six genuine grant processes/files, for 62 product observations; fresh
complete/live/native validation remains required. This does not close #1131.

### Scheduler test-root race correction (2026-10-03, #1131)

The complete Go-absent selection at clean `0d1be28288594354941a07d19f834f2553928bc4`
retains an actual scheduler failure: the twenty-case differential found an
empty file inventory for `launchd_install_over_own_units_succeeds`. The
parallel unsupported-platform test called the same PID-only `run_root` helper,
which recursively removed the first test's live files. Each test now owns a
unique RAII temporary directory; creating or dropping a sibling cannot remove
its evidence, and panic/normal cleanup affects only its owner. A control
writes sibling evidence and checks both its preservation and final owner
cleanup. Original Go observations, every case, file/mode/command comparison,
parallel test execution and all production scheduler code remain unchanged.
All nine scheduler installation tests and thirteen config tests now pass
with Go absent, including sibling-evidence preservation and full original
file/mode/command comparisons. Strict targeted engine Clippy and formatting
pass. The complete updated workflow and native current-source acceptance
still require fresh execution; the earlier failed run is retained.

### Actual native Mac scheduler/config frozen preparation (2026-10-03, #1131)

Run `37154503080` at clean `30eeb38f1e43c8f633d3537818d1de8b96ba9d6a`
now passes the complete Rust workspace on all six native targets. All six
37-case Go artifacts independently verify published ZIP digests, 65 current
source files, eight whole stdout/stderr streams, the complete twenty-case
installation document and unmodified native Go 1.26.6 VCS identities. Both
Mac artifacts (`11285567103` arm64, `11285378517` amd64) are retained with
their original whole documents and manifests. The original Windows CRLF
failures and superseded four-target captures remain recorded above.

Actual Mac installation/config bytes match each other across architectures
and the independently captured Linux outputs; this equality is measured,
not a synthesized platform conversion. Both Mac defaults now use their
native source-bound recordings. The twenty installation cases retain every
file hash/mode and recording-runner command; all six config effects retain
the original semantic and provenance checks. Native manifests also check
all 65 source hashes, exact target and embedded unmodified build identity.
Additional controls reject changed streams and unobserved architecture/OS.
The eight installation and thirteen config tests pass locally with Go absent,
and strict targeted engine/core Clippy passes. Native execution of these new
Mac default paths and the complete updated selection still remain required;
source30e acceptance does not stand in for later code. No migration task or
contract row is promoted by this preparation.

### CLI review frozen-oracle preparation (2026-10-03, #1131)

Actual Go 1.26.6 capture at clean `46f2aec331a49b1d2e6423f15763b67c0fda7eba`
records all eight positional/path/output review cases on Linux amd64. Each
record retains complete exit status/stdout/stderr, original input hash and
unchanged-file effect, plus 171 current Go source/module hashes and the actual
unmodified embedded VCS build identity. Linux default replay verifies those
sources and streams, then executes every original Rust CLI process and raw
comparison, including redaction and unchanged input. Other platforms and
explicit live mode retain actual Go execution; the one-off build now has the
shared 120-second budget and existing bounded file/process cleanup.

Both selected review tests pass with Go absent, including changed/truncated
stderr rejection; strict CLI command-surface Clippy and formatting pass. The
previous 109-test selection also passed at the same clean capture source.
At clean `f34cbb35d0045ca034370063dd3e02851081be44`, both tests also pass
against actual private-root live Go, the exact extended workflow passes all
111 tests with Go absent, and the complete 56-operation native capture pipeline
runs successfully on actual Linux amd64. Current-source all-six native
acceptance remains required. The
native observation pipeline additionally captures all eight real review
process outcomes, for 56 product observations in total. This preparation does
not close #1131, remove Go, or claim unsupported native results.

### Neutral and crypto frozen-oracle preparation (2026-10-03, #1131)

Actual private-root Go 1.26.6 capture at clean source
`aa2dd0f31d30061b5ed66e7638a0dcddde060009` records all 32 timestamp and eight
ordered confirmation-URL inputs. The neutral package now defaults to these
whole-output/input-hashed observations while retaining explicit live mode
with private runtime roots and the unchanged 30-second total budget. Local
default execution with `go` absent from PATH passes all 21 existing harness
unit tests and both differential/control tests, including all 40 observed
inputs and three corrupted-oracle controls. The original 40-case differential
is retained; the extra test exercises corruption detection.

Fifteen additional native Go observations at the same clean capture source
record identity/crypto requests, stdout, stderr and status, including five
tampering failures and real Go reads of Rust writers. Default core identity
and encryption integrations use exact recorded request lookup with complete
stream length/SHA-256 verification. All original 21 integration tests remain;
one added control checks all 15 records, equivalent Rust rejection of all five
tampered envelopes and refusal to answer an unknown key. Two private writer
tests reproduce whole captured envelopes through existing deterministic
helpers, while public encryption retains fresh randomness and round trips.
The Go-free workflow now selects these integrations and the two writer tests
alongside the neutral package. This is not a complete core/CLI/engine freeze.
Executing the exact workflow script locally with Go absent from PATH passed
47 tests: 23 neutral, 17 encryption, five identity and two private writer
tests, with zero failures or ignores. Strict core all-target/all-feature
Clippy, workspace formatting and task-graph checks pass. Native CI remains
pending; these local results do not establish all-six-target acceptance.
The same identity/crypto integrations also pass in explicitly selected live
Go 1.26.6 mode at clean `812cc7ee7c7d6b4021c7259f9cda6923efa2fd98`, with private
HOME/USERPROFILE/XDG/temp/data roots and no operator credentials inherited.

An actual Go 1.26.6 Triage-Service capture at that clean revision records its
four complete classify/rebuttal/fallback/error results and persisted effects
in a 7,986-byte output (`dc632eaa6a15cac34a390d6d3472f22ca0223b628fafcac32adeff29edc02e1f`).
Default service tests verify the raw output manifest and retain the original
eight Go source pins and all four effect comparisons. The five original
tests and one added byte-change/missing-operation control pass with Go absent
from PATH, with strict core Clippy. Explicit live Go mode remains available.
All six service tests also pass in actual explicitly selected private-root
Go 1.26.6 mode at clean `8484efa`. An opt-in native Rust CI input,
`capture_frozen_oracles`, now runs a read-only capture of these four service
operations and all seven projection boundary cases on the existing six
native hosts. The projection's out-of-range Go float-to-int conversion is
architecture-specific; no AMD64 observation is fabricated into ARM64 data.
Captures use clean source/build provenance, private runtime roots, exact
raw streams, existing 120-second build/30-second oracle budgets and reaped
root children; compiler-descendant confinement is not claimed. The new
output directory must be outside the checkout and must not already exist.
Only actual successful captures produce a manifest; CI artifacts still
need native execution and review before incorporation as frozen fixtures.
Before that full native capture run, the preparation also retains the exact
SMTP candidate's staged Windows HTTP fixture repair: one complete request
buffer and the existing bounded reader, with unchanged five-second deadline
and full header/body requirements. This prevents the reproduced split-write
failure retained by #1154 (`37148405763`, job `111276980727`). It changes no
production server behavior and claims no new Windows result before execution.
Native capture run `37150753738` at `6ab9537` records all eleven operations
on both Linux and Windows architectures; both Mac captures and complete
suite acceptance remain pending. Readback exposed Windows CRLF conversion
of the projection JSON input, faithfully recorded as a different source
digest. A real local Git checkout with `core.autocrlf=true` also reproduces
conversion of the captured raw service output. Pin raw captured streams as
non-text and manifests/projection inputs to LF, and use portable source-path
keys for subsequent captures. No stored observation is rewritten to conceal
the original checkout difference; new native acceptance is still required.
Both native Windows full-suite jobs (`111283985890`, `111283985991`) at
`6ab9537` subsequently reproduce those exact length failures: neutral output
6215→6216, recorded stderr 65→66 and service output 7986→7987. Both Linux
full suites pass. The correction preserves all original stream bytes and
all digest/length assertions. A real Rust negative control rejects the
actual Git-converted service output with exit 101; the fixed Git checkout
preserves all 53 then-existing fixture/input files byte-for-byte.

Four downloaded native artifacts (`11283751634`, `11284415392`,
`11283852460`, `11283628107`) verify archive digests, all 49 source files,
all raw stream lengths/hashes, clean embedded Go build/source/native-target
identity and all eleven operations per target. Linux/Windows projection
outputs are byte-identical within each architecture; only `int64_max_payload`
differs between AMD64 and ARM64. The projection family now defaults to those
actual separate whole outputs, preserves all seven original comparisons and
adds byte-change/missing-boundary controls. Both local default tests pass
without Go, with strict core Clippy. Native Mac captures, corrected Windows
suite acceptance and the other runtime-oracle families remain open.

Scheduler generation/legacy detection now also defaults to an actual Go
1.26.6 capture at clean `3a3e25079cc2ebcd1564cadc9ae78e6e85c51168`: six
generator configurations and twelve legacy inputs produce a complete
24,694-byte document (`1e8f3cf39b0ee3de4d9ba8381f1541aa9def47586a6e8ca430146101c59c0b64`).
The parsed result matches the existing fixture without changes. All six
original Rust tests remain selected; one added corruption test rejects a
changed byte and a missing generator case. Seven default tests pass with
Go absent, and strict engine Clippy passes. Explicit live mode uses the
shared bounded runner instead of the former unbounded compiler/background
wait. Scheduler install/config and other families remain open; this local
Linux capture is not new six-target or integrated phase acceptance.

This candidate changes no contract-row status or integrated task completion.
Its Go-free CI definition still needs candidate execution. The other runtime
Go helpers, command-line live interface and switchback runners remain open
under #1131. Go removal stays gated by the actual stable release and seven
days; neither #1131 nor #1133 is closed by this preparation.

The task graph is reconciled against integrated `28e32a1c` and the current
matrix. Phase 5 is complete (`last_completed = 5.4`); task 6.2 also has PASS
evidence. Phases 6–8 are not complete. Closed historical phase issues
#807–#812 do not establish their remaining acceptance gates.

`execution_state.task_issues` maps every unfinished task to the granular
issues of #813; `contract_issue_owners` maps every PARTIAL/TODO row. The
matrix's row status remains authoritative. `active_tasks` means unfinished
implementation/evidence, not a live writer claim. Task 6.1 remains unfinished
because its planned crate-decision artifact is absent; #1119 owns that gap.
`python3 tests/test_rust_task_graph.py` checks this reconciliation without Go,
network calls or fixture regeneration; CI runs the same check.

The accepted CLI-023 manual-confirmation deviation (#809, 2026-09-28) is not
a browser-implementation blocker. #1122 owns its remaining native evidence
and CLI-024 restore rehearsal. The Rust signed-DMG gate now belongs to
#1129, not the closed historical Go signing fix #794. CoreKit #366 closed
on 2026-10-02; the native Windows host-agent acceptance below now resolves #1121. Publication, production
cutover and Go retirement remain separately gated; #1132 requires an actual
stable release and seven days of observation before #1133.

## Native MCP HTTP completion candidate (2026-10-03, #1126)

The full-header comparator already exists in `mcp_http_headers_process.rs`:
ten actual Go/Rust responses compare the status line, every header including
duplicates and raw body bytes. Only a validated canonical Date value is
normalized; deliberate missing/extra/changed/duplicate-header controls fail.
The older MCP-007 row does not describe this merged implementation.

`mcp-http-native.yml` now explicitly selects that comparator and three actual
socket-error controls on all six targets: occupied IPv4, occupied IPv6 and an
unavailable TEST-NET address. IPv6 cannot be silently skipped. Both actual
checked-out Go 1.26.6 and Rust CLIs must reach token creation, fail within ten
seconds, and match complete exit code/stdout/stderr without normalization.

Initial actual Windows amd64 run `37124645649` at `a0984d1c` failed the
occupied IPv4 diagnostic: Rust returned Unix wording while Go returned the
native Windows WSAEADDRINUSE message. Token DACL/rotation/retention and all ten
full-header scenarios passed in the same job `111207382913`. Rust now formats
these two bind-error classes through the system's US-English message API,
matching Go's locale fallback and CR/LF-only trimming, with no OS-code suffix.
The original failure is retained; candidate native results remain pending.

Windows additionally compares complete token owner/group/DACL SDDL, inherited
and owner-only parent behavior, protection/read-only flags, restart rotation,
old-token rejection/new-token authentication, and read-only replacement failure
with exact token/sentinel retention. PowerShell uses framework ACL APIs in a
cleared private environment; commands have thirty-second bounds. Random token
bytes differ intentionally; both must decode to 32 bytes and independently
rotate. No operator data or ACL is changed.

Both Ctrl+C and Ctrl+Break run Go and Rust sequentially in a newly created
private console. Ctrl+Break targets the owned child process group; Ctrl+C can
only broadcast inside that private console, after the controller enables the
child's inherited signal flag and then ignores the signal itself. The runner's
console is never targeted. Exit success and listener closure are required for
each real child. Existing bearer, Origin, timeout and disconnect gates remain.
MCP-007/011/012/013 are PASS on source
`53a0e3684652f0676f02e2994bcd46fbeb9d83b2`: targeted run `37128540191`
and full Rust run `37128575147` passed all six native targets. Both Windows
jobs executed complete DACL/read-only retention and private-console Ctrl+C/
Ctrl+Break; every target executed all three mandatory bind errors and the
complete-header comparator. Task 8.3 remains pending integration of its
recorded task 8.1 and 4.7 predecessors; no dependency is inferred from these
individual row results.

## Official MCP core conformance candidate (2026-10-03, #1126)

The required official conformance gate was measured with upstream
`@modelcontextprotocol/conformance` 0.1.16, immutable source
`21a9a2febd7100d7c17ac1021ee7f2ed9f66a1e0`, its unchanged package lock
(`df89d138b91871a7fb041f8d3923a78b9a1e2d5bdf28ba589fe6f944f30814fe`)
and SDK 1.27.1. At `d9a66fabb031d9c44b515a9d40295839be5af625`,
both actual Go and Rust passed `server-initialize` and `tools-list`, but
failed `ping` with -32601. This earlier failure is retained.

Source `0bc6b051890340822448fa3846c96f70022b79ad` adds the protocol
method in both backends: empty result, existing ID encoding and parameter
validation, no tool dispatch, and silent notifications. It changes no
catalogue entry or existing initialize, HTTP, error or notification frame.
Actual Go 1.26.6 recaptures at `0718834abaea0c78ea22008e28c8f8153ce8a89f`
preserve every previous 670 stdio process observation, agent/provider
cancellation effect and malformed-agent error response; only source
provenance changes. Eight new raw ping cases extend the process corpus to
678. The earlier compiler-host suffix is retained in historical captures;
the recapture records its actual Linux/amd64 host.

At clean `a46bc2c07c0ae0f9f9c1d8ef35dc663b72e65c52`, the pinned official
three-scenario core subset passes for both real backends on native
Linux/amd64 (six SUCCESS checks, no baseline or rewritten assertion).
`scripts/check-mcp-conformance.py` builds source-bound Go with unmodified
VCS metadata and uses the built Rust CLI, private roots and authenticated
loopback servers. Its small fetch adapter supplies only the owned disposable
bearer header, rejects other endpoints and preserves SDK requests/responses.
All owned processes are bounded and reaped; retained receipts exclude tokens
and private server logs. `mcp-http-native.yml` now repeats this pinned core
check in addition to the six existing native HTTP platform jobs.

This is the applicable official core subset, not the entire reference-server
suite: other scenarios require upstream `test_*` tools or unadvertised
resources, prompts, logging, sampling and SSE capabilities. Adding those
reference tools would break the pinned 26-tool catalogue. Current-head CI,
the newly extended corpus on all six native targets, task 8.1 acceptance
review and ordinary integration remain pending; this local result alone
does not complete task 8.1 or task 8.3.

Current-source official CI job `111316975982` in run `37161925589` passes
all six checks at PR head `32d5c6bba4244571428276608024ba896db06961`.
Downloaded artifact `11287877386` verifies its fixed single manifest member,
archive SHA-256, pinned upstream/SDK/Node identity, actual Go unmodified VCS
metadata and virtual merge `034aba85a28aec46d28e76b2d647c1838d28b5a7`
whose parents are that head and main `7a1baac8`.
The full native run `37161978066` retains four Linux/Windows failures in the
non-writing Python oracle acceptance control, before the native Rust suite:
its two inventory assertions still expected 670 instead of the measured 678.
Only those expected inventory counts are corrected; all existing corruption,
compiler/host, missing-case and no-write controls remain intact. Fresh complete
native acceptance on the corrected head remains required.

## Native SMTP campaign gate (2026-10-03, #1118)

The new explicit `NetSmtpTransport` now supplies the previously absent Rust
network sender. The actual Go/Rust campaign differential checks rejected
recipient followed by successful send, exact normalized SMTP/MIME bytes,
persisted events and complete projected state. Eight ordinary protocol
controls match Go; an additional credential-echo control verifies Rust's
required redaction of the raw/quoted/encoded synthetic secret actually
observed in Go's diagnostic. Three private-root Rust STARTTLS handshakes
verify trusted/unrelated/expired chains before authentication.
`docs/rust-smtp-transport.md` records the adapter, bounds and precise evidence
limits. The native workflow selects these controls and retained plan/CLI
suites on all six targets. Local controls and strict linting pass.
At branch source `cb3234c2291c8b243aeed8452e2bfce6f1938868`, targeted
run `37137509483` passed all six native targets, including Intel Mac job
`111244931566`. Actual log records include the complete persisted campaign
differential, eight ordinary protocol cases, one credential-echo privacy
control, three private-root TLS handshakes and retained plan/CLI execution.
DOM-002/CLI-010 are PASS for that scope; task 7.1 awaits final PR checks and
verified main integration. #1119's independent OS-store trust scope remains open.
At `419d60c9`, targeted run `37123597940` passed Linux amd64/arm64 and
Windows amd64/arm64, including the actual transports and retained plan/CLI
checks; its macOS jobs are pending. Full workspace run `37123623125` found a
retained Windows arm64 HTTP fixture failure in job `111204464694`, not an SMTP
mismatch. This branch carries the bounded request-staging repair described in
`docs/rust-port/handoffs/2026-10-03-windows-mcp-staged-request.md`; no timeout or
response comparison was relaxed. The current targeted six-platform acceptance
above supersedes the earlier pending SMTP scope; complete workspace integration
checks remain separate.

## Artifact-level SBOM preparation (2026-10-03, #1128)

The native shadow archive workflow now prepares embedded Cargo inventories
with pinned cargo-auditable and rust-audit-info, records native Go fallback
build info, and binds both executables and archives into CycloneDX documents.
Independent packed-member readback reproduces every inventory and SBOM.
Six local mutation/identity controls and a real Linux ELF extractor smoke pass;
these are preparation evidence only. REL-010 and release/Homebrew rows remain
PARTIAL pending native product archives, signatures, provenance, audit/deny
and published readback. Details: [artifact SBOM handoff](../rust-port/handoffs/2026-10-03-artifact-sbom.md).

## Complete MCP tool byte acceptance (2026-10-03, #1124)

The new `mcp-tools-native.yml` selects all six native targets. It combines
production dispatcher/envelope tests, the real stdio campaign/scheduler cases
and compiled native-agent triage. Current Go recordings add complete bytes
for six fixed-clock calendar/dashboard cases, four stored-reply auto-confirm
responses, and 25 remaining-tool/boundary cases, including the entire embedded
registry and ten corrupt-database errors. The cleanup boundary additionally
proves that manual-file cleanup does not inspect the database. Current-source
hashes are checked before comparing responses; execution uses private roots.

The candidate fixes broker struct field order, integral form-spec numbers and
Go's public `SQLITE_NOTADB` spelling at store open. Rendered Go reporting now
uses its existing optional clock, whose default remains `time.Now`. The actual
Go recaptures of CLI triage, agent-error JSON, scheduler and campaign fixtures
changed only source provenance; all request, response and side-effect bytes
remain unchanged. The source commit is `0d34c46348932b076086730f2adf72a23bef533a`.
The accepted manual browser fallback remains documented under #809.
Native run `37137670713` passed all six targets at
`0980de4edd354b69f77ea5c86a0001ecc6b2d16f` (clean PR merge
`b45637860246dc51ca02a5158326b14bfbf0af01`). Each target actually executes
97 selected MCP tests, all 25 newly complete Go responses, retained stdio
cases and both native-agent tests. MCP-003/004/005 pass their scoped contracts.
Full native workspace run `37137772972` passed all six targets. PR #1153
integrated as `7a1baac8051de92527141b1e6e120b6dd5ddbcb4` after all 39
current-head checks and all nine overall workflows completed successfully.
Issue #1124 is completed for its scoped MCP-003/004/005 acceptance. Task 8.1
remains unfinished under #1126; task 8.4 retains its remaining open owners.
Task 8.5 remains gated by completed CLI/handler wiring and is owned by #813. See
`docs/rust-port/handoffs/2026-10-03-mcp-all-tools-bytes.md` and the coordinator
specification/quality reviews; no independent-agent review is claimed.

## Integrated LLM fixture lifecycle acceptance (2026-10-03, #1142)

The original `b2d282e` Windows amd64 failure in run `37045269185`, job
`110965028842`, remains retained. Diagnostics merged by #1143 proved request
counts and redacted outcomes but did not identify the original scheduler event.
Code inspection identifies a lifecycle defect: the accept timer starts before
`create_with` finishes constructing the client, so unrelated client preparation
can consume the budget intended for its three HTTP attempts. The production
retry policy is two then four seconds; successful diagnostics record requests
at approximately zero, two and six seconds.

The fixture now binds first, constructs its actual client, then starts serving
with the same fifteen-second accept lifetime. Separate setup/active timings,
status, received/expected counts and error class remain observable; no prompts,
keys or response bodies are logged. Accepted sockets use bounded read/write
waits instead of unbounded reads. Production retry, TLS trust, LLM configuration
and pinned Go fixtures are unchanged.

A distinguishing executable control uses the same short budget on both clock
origins: delayed preparation exhausts the original origin with zero requests,
while arming at readiness returns the required complete 403 response. A second
control sends one of three requests and must fail with an exact one-request
count within its short deadline, with the thread joined. The seven retained
provider cases still require all three actual production attempts, exact path,
Go error class/message after required credential redaction, and unchanged Go
fixture regeneration. All ten local tests pass. Source
`71f36604856f49ebc249fd1088d2c014e36df200` passed all six targets in targeted
run `37137392379` and full native workspace run `37137564400`. Archive run
`37137392415` passed all six native binaries and same-run six-archive verification
job `111275349421`; all 46 final PR checks are accepted. PR #1152 merged as
`c797d217b97197d0dbfb56741ecc95c5be001a77`, and #1142 is closed by readback.
This proves the mechanism and repair; it does not fabricate a timestamp for
historical client setup. Following candidates include this verified main
predecessor and require their own current-head checks before integration.

## Native Windows consent gate (2026-10-03, #1123)

`consent-windows.yml` selects actual amd64 and arm64 execution. Four private
filesystem cases compare checked-out Go 1.26.6 with Rust: inherited DACLs,
an owner-only parent, a read-only directory and failed replacement of a
read-only token. The comparison covers complete filename sets, exact bytes,
owner/group/DACL SDDL, protection flags, read-only state and unrelated sentinel
preservation. The oracle records and verifies the four source hashes.
Windows Go chmod changes read-only attributes and inherits the parent's DACL;
0700/0600 are not translated into an owner-only Windows DACL by this gate.

The checked-close control releases its owned file once, then calls the same
production `CloseHandle` error boundary with a null handle. Windows must
return real `ERROR_INVALID_HANDLE` (6); the atomic-write operation must
propagate that failure, skip chmod, retain the old token and sentinel, and
remove its temporary file exactly like the retained Go close-failure fixture.
This is an actual kernel-error control through the operation-local seam;
it does not manufacture an invalid owning `File` or double-close a handle.
Initial amd64 run `37120919444` at `576c07f6` hit the bounded 30-second
PowerShell ACL observation deadline in its cleared private environment.
The observation script now uses framework ACL APIs and direct bounded JSON
output, avoiding cmdlet module discovery, with explicit stage diagnostics.
This is a pending harness repair; the original failure is retained.
Native run `37121535781` at `21a4fca0cebe4d3956a8c62090b91948672066d2`
passed amd64 job `111198458769` and arm64 job `111198458869`: all four
complete-tree byte/DACL comparisons, both actual-kernel close-error and
open-old-file controls, twelve Windows API tests and three portable cleanup
controls. The environment child helper is an expected skip, not evidence.
ID-005 and task 4.7 are PASS; existing native Unix evidence is retained,
with unchanged Unix production behavior. Complete final PR CI remains a
separate requirement before merge.

## Native Windows host-agent gate (2026-10-03, #1121)

`host-agent-windows.yml` now selects the real Windows HTTP disconnect/reap
and PATHEXT differential tests on amd64 and arm64. Each disconnect case runs
the checked-out Go CLI and Rust CLI with a locally built synthetic executable;
an owned Windows process handle establishes that it was live before the
disconnect and terminated afterwards, with a healthy MCP server and no saved
classification. The PATH cases record source hashes from the actual Go helper
and `exec.LookPath`/`AgentClient.IsAvailable` observations in disposable roots.
No operator profile or paid agent is used. DOM-008/DOM-008A are PASS on
source `54d8c138a9d7aa0811701c736faa0575c08d4558`: targeted run
`37117363917` passed both Windows jobs (amd64 `111186642692`, arm64
`111186642789`), including all nine lookup cases and malformed-stderr MCP
replay. Full native run `37117425498` passed all six OS/architecture targets,
including provider descriptors, retries/errors and cancellation.

The first native run `37115382421` at `cd2e68ee` established both Windows
disconnect/reap controls, but the PATH comparison failed on amd64 job
`111181097154` and arm64 job `111181097006`: Go rejected an extensionless
`claude` with `PATHEXT=.EXE`, while Rust reported it available. The candidate
builder now follows Go's extension rule; the portable filesystem regression
also checks the contrasting empty-extension-list case. The original failures
remain evidence; both final native lookup comparisons reject the extensionless
file with .EXE and accept it with an empty extension list. Task 6.5 now meets
its recorded contract and source-bound descriptor/provider gates. The
independent CoreKit #366 dependency and EraseMe #1112 are closed.

## MCP parser diagnostic hardening (2026-10-02, #1125)

The shared iterative protocol scanner now reports Go-compatible diagnostic
context directly. The removed serde-based heuristic confused array separators,
hex escapes, multiline positions and the first error in a malformed value.
The pinned Go 1.26.6 process oracle now has 670 cases: the existing 138 cases
are byte-unchanged, with 20 named syntax/EOF probes and two 256-byte sweeps
added for object keys and escapes. Both buffered and one-byte-read streams
consume the source-derived observations; the real CLI compares exit/stdout/
stderr without normalization. Initial execution reproduced the array-separator
divergence before the production fix; all 670 process cases and 31 parser/
stream tests pass locally afterward. Strict all-target CLI Clippy also passes.
The handler already injects an unreachable local model endpoint; no real model
or paid provider is used by these checks.

Independent review additionally exposed a pre-existing live-pipe boundary gap:
all top-level scalars were dispatched before Go's required lookahead/EOF.
A readiness-synchronized probe of both actual CLIs confirmed it for strings,
null, booleans and numbers. The stream adapter now delays only boundary-ending
scalars; objects/arrays remain eager. `stdio_primitive_waits_for_lookahead_or_eof`
checks exact read/write ordering without sleeps and failed before the fix.
The same two-CLI probe matches afterward for all nine tested token forms.

MCP-008/MCP-015 are PASS on source
`91af942ac72449771871771e02aa5eaae64e9c6e`, merged by #1135 as
`adeb6b12341a38873a40fefec25621e845d7756a`. [Native run 37007678845,
attempt 2](https://github.com/danieljustus/symaira-eraseme/actions/runs/37007678845)
executed the named stdio process corpora successfully on all six targets.
The original Intel HTTP fixture port collision remains separately tracked in
#1136. [Hardening run 37007682140](https://github.com/danieljustus/symaira-eraseme/actions/runs/37007682140)
passed Miri, all six configured fuzz targets and MCP mutation testing:
4,536,509 MCP-parser fuzz executions in 121 seconds (120-second budget);
331 mutants tested, 298 caught, 12 unviable, 21 timeouts, zero misses.
Timeouts are not killed mutants; bounded fuzzing is not exhaustive differential
fuzzing. The injected handler endpoint remains hermetic. Historical captures
remain valid only for their recorded revisions. Release/cutover and unrelated
MCP tool/Windows contracts are not promoted. Task 8.2 has its merge-gate
evidence; the handler-wide hardening task 8.5 remains owned by #1124 until
that remaining tool/error-path scope is complete.

## MCP parity checkpoint (2026-09-30)

- MCP-015: #1110 merged as `916bbfccedfc21ddaab28b6cf917dc1774ac4aeb`;
  #1108 is closed. Exact-head native run `36635311862` at `96db859e`
  passed on all six targets. Each job log confirms fresh pinned Go capture
  of all 138 cases without writes and the Rust mutation process replay.
  Archive proof `36635317091` passed all six native builds and archive/checksum
  validation after retrying a Go dependency-download network timeout.
- MCP-007 HTTP response parity: #1111 merged as
  `fd63a368ffa660247134b41a7f44da6b32553a7b`. Both named HTTP comparator tests
  passed on all six targets in `36632119595` at `aa882843`. #1109 is
  closed after integrated six-target run `36934832118` at `28e32a1c` revalidated both complete-header comparator and its mutation controls on every target. Broader parser fuzzing belongs to #1125; remaining native ACL/signal/bind controls belong to #1126.
- MCP-005A/DOM-008: #1113 merged as
  `726156748d886029e54509cf2181f9639d9cba0d`. Native run `36637116514` at
  `60bb1ec4` passed all six targets. Both Windows job logs confirm the real
  Go/Rust malformed-agent-error comparison and live stdout/stderr overflow
  controls. #1112 remains open; merge alone does not close its remaining scope.
- Post-merge Rust CI, native target proof, hardening, general CI and CodeQL
  passed on both integrated commits: Rust CI runs `36673177006` (#1113)
  and `36674636824` (#1110). Independent final reviews were PASS_STATIC;
  local workspace, strict Clippy and formatting passed on the candidates.
- Production behavior is unchanged. No release, cutover or Go removal is
  authorized. Continue from the remaining contract prerequisites rather than
  reopening these verified parity checks.

## Acceptance gates (all four required per slice)

```
cargo nextest run --workspace
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
go vet ./... && gofmt -l .
```

## Contract

The CLI corpus `rust-tests/parity/cases/cli/behavior.json` (175 recorded Go
cases) is the contract. `is_exact_case()` in
`crates/symeraseme-cli/tests/command_surface.rs` splits it: selected cases are
compared byte-exactly, the rest only assert the deferred stub fails closed.
**Migration progress = cases moved into the selected set**, not files ported.

Local selected: **175 · deferred: 0** on the integrated local tree. The real
`poll-inbox` adapter, invalid `--since`, and both reply-triage commands replay
source-bound Go cases. The LLM-backed cases reproduce Go's missing-key error
without a paid call. A
green CLI corpus does not close JSON-state, review or native platform gates.

## Integrated continuation (2026-09-23)

- CLI-024 migration engine and its 500-case Go JSON oracle are committed.
  Focused macOS and Linux aarch64 tests passed; native Windows and an exact-head
  remote matrix remain open. The earlier 432-test workspace result applies to
  that CLI-024 candidate, not automatically to this later integrated head.
- CLI-025 polls a real IMAP adapter with 175 recorded Go CLI cases, all selected
  after the LLM transport integration. Filesystem generators, populated manual tasks,
  events and grants, and source-bound malformed MCP stdio/process cases are
  integrated and exercised on macOS.
- MCP HTTP starts a real Rust server with bearer auth, Origin and bind policy,
  body limits, token rotation and signal drain. The integrated macOS
  `mcp_http_process` plus `mcp_stdio_process` gate passed 12 tests; native
  Linux/Windows process behavior remains open.
- The Swift app's opt-in Rust process test launches the HTTP binary, checks an
  unauthenticated 401, token-backed tools/list and `list_brokers` calls, and
  verifies shutdown. This exposed two existing app response-shape defects,
  fixed in `026c3839`. The Swift suite passed 41 tests with the Rust binary.
- IMAP modified UTF-7 mailbox names and outbound MIME bytes replay pinned Go
  fixtures. Default IMAP TLS now loads platform roots, including Windows
  LocalMachine ROOT; the follow-up passed independent static review and five
  focused macOS IMAP tests at `c7954258`. Native Windows chain policy and a
  custom macOS Keychain root remain unverified. No paid LLM or SMTP provider
  was called.
- The four Go tick histories now replay through Rust scan, apply, persisted
  scheduler events and projections (`efc718b`); the two Go conformance tests
  and four Rust tick-apply tests pass. Core manual-task HandleList/Show/Complete/
  Cleanup orchestration is integrated at `96da417f`, independently reviewed,
  with eight source-bound Go handler cases and nine focused integrated Rust
  tests passing. Positive tick CLI actions now retain Go's capitalized struct
  keys and declaration order (`ef8ba9c`); matching Go/Rust wire tests pass.
  Native filesystem evidence remains open.
- The offline release checker at `4fa11368` verifies six current Go snapshot
  archives, exact root files, member uniqueness/type and SHA-256 entries.
  Four malformed-archive controls and the real snapshot pass. This establishes
  the existing package contract; the release workflow still packages Go.
  Offline Cargo audit/deny and a source CycloneDX inventory passed, while Rust
  release artifacts, artifact SBOM/provenance, signing and notarization remain
  unverified.
- Local Rust archive staging now produces the six required archive names,
  root members and checksums from explicit caller-supplied binaries (`1f63e692`).
  Its placeholder-binary test passes the offline verifier; native Rust binary
  format, runtime, signing and publication are not established by that test.
- The manual `.github/workflows/rust-prerelease.yml` gate now builds on native
  macOS, Linux and Windows arm64/amd64 runners, checks binary startup/linkage,
  stages the six Rust binaries with the legacy archive contract, and verifies
  the resulting archives and checksums. The exact-head `79ef6a85` PR run passed
  all six native builds and the archive verifier; REL-001..004 remain partial
  because `release.yml` still packages Go and is unchanged.
- Windows release builds select Rust's static CRT feature and fail if `dumpbin`
  reports a dynamic CRT or non-system DLL dependency. The Go release contract
  sets `CGO_ENABLED=0`; the `79ef6a85` PR run verified both Windows native targets.
- Core `GetPlan` now serves both CLI and MCP reads. The independently reviewed
  source-bound Go campaign oracle at `131fb6b4` checks all seven pinned plan
  timestamps before and after execution, plus local no-send web-form/manual
  fallback and missing-email-sender transitions. Three integrated Rust tests
  passed. MCP `execute` now has isolated local replay; broader execution paths
  remain open.
- `review` now has eight live Go/Rust CLI differential cases for positional and
  `--path` aliases, text/JSON modes and failures (`fe9250a`); the input file is
  unchanged and four successful paths must redact its address.
- Core reply classification and rebuttal service orchestration is integrated
  (`4af87d9d`). A source-hash-pinned Go oracle compares full results, saved
  reply columns, ordered event records and stable projections with injected
  local clients; 19 focused Rust tests passed after integration. Both CLI
  wrappers replay 15 local-agent Go cases, including save flags, aliases and
  errors (`23a44ff`, `e1450c5`). MCP `classify_reply` and `generate_rebuttal`
  replay the live Go handler through the Rust stdio process (`f690b8bd`). Four
  llmkit-backed chat providers now use Rust transports with five fake-local
  HTTP Go cases (`d355e3b2`); echoed keys are redacted from errors and Debug
  output (`17607327`). No paid provider was used. Native target runs remain open.
- The real Rust MCP stdio process now replays 72 valid Go initialize ID/params
  cases as adjacent JSON values (`7cc76c8a`) with bounded I/O and exact output;
  its focused integrated test passed. Ten malformed/truncated cases remain
  source-bound. Six malformed parse cases and four size/depth boundaries also
  replay Go 1.26.6 process bytes (`6ccd1a4`). The in-process libFuzzer target
  now includes the production protocol/stream modules directly, seeded from all
  ten source-bound malformed-stream cases; a current-source macOS arm64 run
  completed 330,402 executions in five seconds without a crash.
- MCP scheduler install, status and uninstall now replay source-bound Go cases
  through the real Rust stdio process (`c485e852`). Cron runs against a private
  crontab; launchd and systemd installs use isolated HOME roots and fake service
  commands. All five integrated stdio process tests passed. Native target runs
  and more scheduler failure paths remain open.
- MCP `plan_create` and `execute` now replay five source-bound Go cases through
  the real Rust stdio process (`aea9f15`). The cases cover the default and a
  corrupt explicit profile, dry-run preview, denied live execution, and a
  consented local manual fallback without a network sender. The first
  integrated replay exposed Go struct field ordering in the content text; the
  focused stdio process suite then passed 6/6 and the CLI package passed 96/96.
  Consent directory resolution now uses `USERPROFILE` on Windows and propagates
  missing-home errors; native Windows execution remains unverified.
- Reporting JSON and HTML now have raw Go byte oracles on isolated stores
  (`29d48317`). The zero-campaign CLI artifact, one-campaign export, and
  two-campaign export with an empty first campaign cover the HTML boundaries;
  the focused integrated core tests passed 6/6 and filesystem replay passed 1/1.
- A new PlanCampaign/GetPlan oracle (`956bd9db`) compares raw Go/Rust result,
  request, campaign and persisted event bytes. It exposed Go's sorted JSON map
  order in `payload_json`; the shared event append now matches it (`c0d6dd19`).
  The focused integrated plan tests passed 3/3.
- The integrated tree at `29d48317` passed 481 workspace tests (two helper
  tests skipped) with `TMPDIR=/tmp`; `go vet ./...` and `cargo fmt --all --check`
  passed. A preceding run with a longer temporary path hit the macOS Unix
  socket path limit in `identity_profile`; that test passed with `/tmp`.
- The later integrated tree at `79f309f` passed 491/491 macOS workspace tests
  (two child-entry helpers skipped) with `TMPDIR=/tmp`. A test-only host-agent
  deadline was raised after one load-dependent fake-process timeout; its
  focused replay passed. At `20ef924`, `go vet ./...`, tracked-Go `gofmt -l`,
  `cargo fmt --all --check` and strict offline workspace Clippy passed. A
  macOS arm64 Rust release binary built at that head, ran `version`, and linked
  only Apple system frameworks/libraries; the other five native release targets
  and exact-head target matrix remain open.
- On native Linux aarch64, the offline `20ef924` workspace check, 176 Rust
  unit tests (one ignored), frozen 175-case CLI replay and focused LLM tests
  passed. The later `a022c169` fake-sender campaign oracle passed 1/1. That
  container had cached Go 1.26.8 rather than the pinned 1.26.6 and lacked
  rustfmt/Clippy; it is scoped Linux evidence, not a complete final-head
  matrix. Logs are under `target/linux-aarch64-latest-20ef924.log` and
  `target/linux-aarch64-campaign-a022.log`.
- The source-bound campaign oracle now also executes a local two-request
  batch: one fake email send fails and persists `SEND_FAILED`, the next
  succeeds and persists `SENT`. The raw Go/Rust results, events and projections
  match at `a022c169`; the focused macOS test passed 1/1. Real provider and
  broader CLI wrapper behavior remain open.
- MCP `auto_confirm` now reads the newest stored reply, previews a trusted
  link on dry-run, and creates the Go-equivalent durable manual task when a
  clicker is unavailable (`17daf2c5`). A source-bound Go MCP process oracle
  and the focused Rust test compare the response and SQLite effects, including
  a NULL-snippet/no-link failure note. No browser click or network request is
  attempted. The production CLI and MCP campaign callers keep the newly
  injectable fake email sender unset.
- An offline formula gate checks the current separate Homebrew tap formula's
  four versioned macOS/Linux archive URLs, SHA-256 syntax, install command and
  version test; Ruby syntax also passes. It cannot verify hashes against Rust
  release artifacts or perform a local install while those artifacts do not
  exist.
- Final local macOS gate on `7151138`: 491/491 Rust workspace tests passed
  (two helpers skipped); strict all-target/all-feature Clippy and Rust format
  passed; full `go test ./...`, `go vet ./...` and tracked-Go format passed.
  The opt-in Swift suite ran against that exact Rust debug binary and passed
  41/41 tests, including the live HTTP/auth/shutdown integration case. The
  release build on the same commit produced a Mach-O arm64 executable,
  `version` returned `symeraseme 0.13.0`, and `otool -L` showed only Apple
  system frameworks/libraries. Logs: `target/macos-nextest-7151138.log`,
  `target/go-test-7151138.log`, `target/swift-test-7151138.log`, and
  `target/macos-release-7151138.log`. These runs do not satisfy the missing
  native Windows or complete exact-head Linux matrix.
- The `7151138` dependency gate passed offline: `cargo audit --no-fetch`
  scanned both Cargo lockfiles, `cargo deny --frozen check all` passed, and
  Syft produced a CycloneDX 1.7 source inventory with 1,307 components at
  `target/sbom-source-7151138.cdx.json`. Artifact SBOM and provenance remain
  unverified without the complete native Rust archive matrix.
- Three additional local-only Go HTTP observations now cover LLM 429 retry
  exhaustion, malformed success JSON and empty choices (`3d74a200`). The
  integrated Rust transport test passed 5/5; no paid provider was contacted.
- CLI and MCP now open the configured encrypted store and explicitly close it,
  propagating finalization failures (`a4757a66`, `60a1656a`). The two sandboxed
  macOS arm64 switchback runners each passed six steps with a Go 1.26.6 binary
  and the integrated Rust release binary: Rust read existing plain/V3-encrypted
  state, wrote one request, and Go read the post-Rust state. The encrypted run
  also read through Rust MCP stdio and retained its V3 envelope. Evidence and
  artifact hashes are under `/tmp/eraseme-plain-switchback-20260923-05` and
  `/tmp/eraseme-encrypted-switchback-20260923-06`;
  `rust-tests/parity/{plain,encrypted}_store_switchback.py` is the executable
  gate. Native Linux/Windows and real restore/cutover remain open.
- Current integrated code candidate `49eb79a8` passed 493/493 macOS arm64
  workspace tests (two child-entry helpers skipped), strict all-target/all-
  feature Clippy, Rust format, Go 1.26.6 `go test ./...` and `go vet ./...`,
  tracked-Go format, and all 41 Swift app tests against the Rust debug binary.
  The six-step plain and six-step encrypted Go→Rust→Go switchbacks passed again
  using the current Rust release binary and pinned Go 1.26.6; reports are in
  `/tmp/eraseme-{plain,encrypted}-switchback-49eb79a`. Logs are in
  `/tmp/eraseme-{nextest,clippy,go-test,go-vet,swift}-49eb79a.log`.
- On that code candidate, both macOS arm64 and x86_64 Rust release binaries
  launched with `symeraseme 0.13.0` and linked only Apple system libraries;
  the x86_64 SQLite portability smoke passed under Rosetta. The arm64 binary
  SHA-256 is `d3ea067a53e63916bd689c99efa5f7224f670dce668f167abea861c898de1043`;
  x86_64 is `c18f401c9d22ced554c717346ae4a200523d1a3764be8b862f9af8437397a86b`.
  Windows GNU workspace all-target check and strict Clippy passed by cross
  compilation after a two-line Windows lint repair. These are not native
  Windows runtime tests. Offline Cargo audit (both lockfiles), deny, the
  six-placeholder archive gate, and current Homebrew formula syntax/contract
  passed; no Rust release archive matrix or publication was produced.
- The earlier parallel macOS x86_64 run at `49eb79a8` hit two fixed 10-second
  process deadlines under load. After serializing the suite and building Go
  oracles for the Rust target architecture, the full `34fe0ef3` run passed
  493/493 under Rosetta (two helpers skipped); see
  `/tmp/eraseme-nextest-macos-x86-serial-34fe0ef.log`. Rosetta is executable
  x86_64 evidence, not a native Intel host.
- Current integrated code `f7f6f72c`: the full macOS arm64 workspace suite
  passed 496/496 (two helpers skipped), including source-bound local 403 and
  404 LLM failures and six fixed-clock MCP reporting cases. The 403/404
  fixtures regenerated byte-for-byte with Go 1.26.6. `go test ./...`,
  `go vet ./...`, Rust format, strict all-target/all-feature Clippy and the
  production Go coverage gate passed; coverage is 78.22% (6,939/8,871).
  Logs: `/tmp/eraseme-nextest-f7f6f72c.log`,
  `/tmp/eraseme-go-{test,vet,coverage-prod}-19c20f50.log` and
  `/tmp/eraseme-clippy-19c20f50.log`. The MCP clock oracle runs from an
  isolated temporary profile; a hostile inherited data path and project
  config remain untouched. The complete macOS x86_64 suite under Rosetta
  also passed 496/496 (two helpers skipped) at `f7f6f72c`, serialized to
  avoid load-dependent process deadlines; log:
  `/tmp/eraseme-nextest-macos-x86-f7f6f72c.log`. A later metadata-only
  repair (`92a573ba`) changed the scheduler/campaign oracle source revision
  from an isolated-worktree commit to the byte-identical, reachable main
  commit `3b61859f`; no fixture case or side effect changed. Its focused
  scheduler/campaign source-bound replay passed 2/2 at `92a573ba`.
  The Swift app suite passed 41/41 with the Rust debug binary, and strict
  macOS x86_64 and Windows GNU all-target Clippy passed. Logs:
  `/tmp/eraseme-mcp-source-revision-92a573ba.log`,
  `/tmp/eraseme-swift-92a573ba.log`, and
  `/tmp/eraseme-{clippy-macos-x86,windows-clippy}-92a573ba.log`.
  On exact `92a573ba`, native Linux aarch64 passed 497 workspace tests across
  55 suites (zero failures, two intentional ignores), strict Clippy, Go/Rust
  format, `go test ./...`, `go vet ./...` and production Go coverage: 78.16%
  (6,934/8,871; 75% gate). The debug CLI launched as AArch64 ELF version
  0.13.0 with resolvable system libraries. Logs are under
  `/tmp/symeraseme-native-gate/logs/` with `92a573ba` in each filename.
  Native Windows and Linux amd64 remain unavailable without runners for this
  unpublished checkout; Rosetta does not replace a native Intel macOS host.
  No release, cutover or publication is authorized.
- The six-step plain and six-step encrypted macOS arm64 Go→Rust→Go
  switchbacks passed again using a Go 1.26.6 CLI built from `92a573ba`
  and the existing Rust release binary. Rust production sources are unchanged
  since that binary was built at `49eb79a8`. The disposable runners retained
  their source/artifact identities and step evidence in
  `/tmp/eraseme-{plain,encrypted}-switchback-92a573ba/report.json`;
  neither run touched a production store or performed a cutover.
- Linux aarch64 disposable switchbacks now pass six plain and six encrypted
  Go→Rust→Go steps using Go 1.26.6 and Rust 1.98.0 binaries built from clean
  `d333ed84` (production code unchanged from `92a573ba`). The Linux runner
  confines each CLI/probe child with private mount, network and PID namespaces,
  read-only host shares, Landlock ABI 4, a dropped uid and no-new-privileges.
  Seven focused controls pass, including a no-namespace fail-closed case;
  all eight runtime denial probes pass. The host-share (`virtiofs`) and
  guest-local (`ext4`) write markers remained byte-identical and were removed.
  The source/build manifest and copied reports are in
  `/tmp/symeraseme-native-gate-host/switchback-linux-d333-logs/`.
  On the integrated `cb21489` runner, macOS plain and encrypted switchbacks
  passed six steps each in `/tmp/eraseme-{plain,encrypted}-switchback-b6a43136-r2/`;
  its focused controls passed six cases with the Linux-only case skipped.
  The one-line macOS probe fix changed no Linux behavior; Linux focused
  controls passed again after that fix. Native Windows switchbacks and a
  real user-data restore remain open; no production cutover occurred.

## Open tasks (run of 2026-09-22)

| Task | Cases | Branch | State |
|---|---|---|---|
| CLI-020 | `operate-auto-confirm` + `operate-migrate` (CLI-023) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`) |
| CLI-021 | `operate-review`, `operate-run-web-form` | `rust/cli021-misc` | **merged** — #1022 (`c511736b`) |
| CLI-022 | `operate-mcp` | `rust/cli022-mcp` | **merged** — #1023 (`4c0236fb`) |
| CLI-023 | `operate-migrate` (validateRoots scope) | `rust/cli020-triage` | **merged** — #1024 (`7d02cc58`); engine behind validation stays fail-closed, see Known defects |
| CLI-024 | migration detection, report, backup/state | local `main` | **integrated locally** — macOS and focused native Linux pass; native Windows and exact-head CI remain open |
| CLI-025 | `operate-poll-inbox` | local `main` | **integrated locally** — real IMAP adapter, source-bound CLI replay; corpus now 175 selected/0 deferred after LLM transport integration |

## Historical continuation (2026-09-22, CLI-024)

- Mode/status: execute / local verification complete; native CI/publication pending.
  Coordinator: `.worktrees/cli024-integration`, `rust/cli024-integration`.
  Dirty candidate is based on `afa22cb9`; reviewed input manifest is
  `target/cli024-review-repair-inputs.json`, SHA-256
  `935002f88832172518df1179ed194e3e700ff5800a066935885ba97aa6269488`.
  Source/test/oracle inputs match the completed static verdict. Shared main remains
  clean at `8986a3db`; no remote publication, Go deletion, release or cutover.
- Reconciled results: `proc_531054572d25` returned **CHANGES_REQUIRED** for
  Windows case-aliased overlap, Windows readonly loss, and scheduler error
  context. Repairs and regression tests are implemented, not yet accepted
  natively. `proc_6981abee04ad` delivered the JSON decoder and genuine 500-case
  production-Go oracle; the coordinator transferred 11 new files byte-identically
  and applied four scoped integration hunks, preserving the safety repairs.
- Local focused evidence: all four generated/native scheduler failure scenarios
  match immutable Go `4e582f28`; all Unicode scalars match the Go 1.26.6 fold
  capture (1454 mappings, 210 exact ranges). JSON writer evidence and its raw
  observations are retained in `rust-tests/parity/oracle/migration-state/`.
  Parent engine tests passed; the first aggregate stopped on a test-only
  non-octal permission literal, now corrected. Do not inherit the old clean-base
  428-test result as final-tree acceptance.
- Final macOS aggregate `proc_cd6f87dde9ab`: **exit 0**, 432 workspace tests
  executed/passed, two child-entry helpers skipped; fmt, strict all-target/all-
  feature Clippy, fresh 500-case Go JSON check and diff validation passed.
  Both safety check entrypoints accepted valid copies and rejected corrupted
  copies without rewriting them. Doctests selected zero tests. The earlier
  mutation-copy Git-prefix error was corrected with a disposable local shared
  clone; no product/fixture expectations changed. All 1539 reviewed input hashes
  still matched after this run.
- Final review `proc_650668909fd5` completed with **CHANGES_REQUIRED**:
  its 1539-file manifest matched before/after; prior production findings were
  resolved statically, but the Windows readonly regression checked the wrong
  backup path. The sole subsequent source/test change is that assertion at
  `migration_review_regressions.rs:144`, now relative to `report.backup_dir`
  under `source/config.toml`. Production and oracle bytes are unchanged.
  Bounded follow-up review `proc_95d4e6d971c2` is **PASS_STATIC**, with all
  1539 input hashes checked before/after and no unresolved static findings.
  Fresh macOS aggregate `proc_a256a9044825` also exited 0: 432 tests passed,
  two helpers skipped, strict workspace Clippy and fmt passed. Log:
  `target/cli024-after-review.log`. This is post-test-repair evidence, not
  inherited approval from the earlier candidate.
- Focused native Linux attempt 4 is **PASS**. Earlier attempt 3
  (`proc_b0b109a67d51`, `target/cli024-linux-3/run.log`) identified **EMFILE**
  in unchanged `symeraseme-core/build.rs:47`: the container's measured soft
  descriptor limit is 1024, while registry collection retains 1277 broker
  handles plus other assets/traversal handles. Tracked as GitHub issue #1034;
  no production change or weakened source-safety check was made.
  Attempt 4, `proc_46cc9db786d5`, uses only a container-local
  `--ulimit nofile=8192:8192` adjustment, still offline and non-root. Its
  source archive, runner copy, identity and full log are retained separately
  under `target/cli024-linux-4/`. Prior failed artifacts remain untouched.
  It exited 0 on Linux aarch64 with Rust 1.98.0: five tests passed across four
  test binaries, including all 500 declared JSON observations, distinct actual
  non-UTF-8 backup names, four scheduler failure observations and all Unicode
  scalars. The old engine first reproduced the expected scheduler-context
  failure; the restored candidate passed. All 2035 archived input hashes were
  checked; all 1539 review inputs match the live candidate. Only the two
  non-build handoff documents changed after capture. This is a targeted Linux
  runtime gate, not the complete native workspace/CLI matrix.
  Local Windows cross-check failed in `ring` because `assert.h` is unavailable,
  before checking engine target code; **native Windows remains unverified**.
  No owned review/test process remains pending. Next external gate requires a
  committed/published candidate and `rust-ci.yml` workflow dispatch on that exact
  head; its native job runs on Linux/macOS/Windows but is skipped on PRs. A PR
  supplies the separate fast/security/coverage gates. Publication authorization
  has not been inferred from these completion notices. No merge/release is
  authorized here. CLI-025 remains dependency-gated.
- Delayed notifications reconciled: Docker pull `proc_ae3212f3572a` succeeded
  (image digest retained in the Linux identity); `proc_4815d0401e2e` is failed
  Linux attempt 2, not a new run. Engine worker `proc_068e417c7478` exited 0;
  its retained commit `474751db07bc68ef579547a012cf338301c28b8d` is already
  integrated as `afa22cb9`. Do not re-integrate or restart that worker.
Loaded this continuation: `guard-repo`, `go-rust-port-parity` plus workflow,
contract-matrix and differential-testing; `code-editing`, `autonomous-coding-agents`,
`parallel-repo-agents`, `evidence-gated-testing`, `port-contract-engineering`,
`python-go-port-parity`, and this skill's worker-dispatch/porting-pitfalls references.

## Prior next action (parked 2026-09-22, second pass — stopped early by user)

**CLI-024 — `migrate` engine** (Detect + dry-run report + mutating path in
`internal/migration/migration.go`, 907 lines): new recorded CLI cases for the
dry-run scenarios (extend `scripts/generate-go-oracle-fixtures.sh`, bump
`expected["cli"]` and `command_surface.rs` 166/163/3 in the same change); the
mutating path is already pinned by the filesystem `migration` case in
`rust-tests/parity/cases/filesystem/manifests.json` (backup/state/manifests +
stdout/stderr sha256) — no Rust replay consumer exists for that case yet, so
wiring the replay is part of the slice. Planned worktree `.worktrees/cli024-*`
@ `305b394b` (git-ignored); worker dispatch prepared but not sent.

Verified 2026-09-22 (second pass): `git diff 4e582f28 HEAD -- internal/migration
internal/scheduler` is empty (fixtures pin current Go behavior); the Rust
engine scheduler already exports `generate`, `detect_legacy_python_unit`,
`detect_platform`; PR #1031 merged as `305b394b` (windows clippy fix). Known
unowned WIP left untouched: detached worktree `.claude/worktrees/determined-lewin-d87b4f`
@ `52e594eb` ("manual-tasks list task objects in Go struct order", clean, not
in main — superseded by #1018; salvage or drop in a later cleanup).

Run of 2026-09-22 closed: integrated HEAD `7d02cc58`, corpus selected
163 / deferred 3, focused corpus test + clippy + fmt + vet + gofmt all
exit 0 on that revision. Ready non-blocked queue: empty — next run has
nothing to start unless an externally blocked row unblocks (llmkit
transports, IMAP transport) or the migrate engine gets its own slice with
Go fixtures.

Wave 1 note: all three wave-1 workers died on HTTP 429 (Codex quota, ~9 h
reset) after ~13 s with no commits; the coordinator implemented every slice
directly in the slice worktrees, per the dispatch contract. Treat an empty
worktree/branch as quota loss, not a failed slice.
Loaded this session (do not re-load): `go-to-rust-migration`,
`go-rust-port-parity` + `references/workflow.md`, `guard-repo`,
`autonomous-coding-agents`, `parallel-repo-agents`,
`go-to-rust-migration/references/worker-dispatch.md`.

## Historical deferred-case queue

| Case id | Subsystem | State |
|---|---|---|


| `operate-generate-dashboard/-report/-scheduler` | generators | done — #1021 |
| `operate-generate-rebuttal`, `operate-classify-reply` | LLM | integrated locally — real Rust llmkit chat transport and exact missing-key Go CLI replay (`e8a8c96`) |
| `operate-migrate` | migration engine | implemented — #1024, validateRoots scope only (engine fail-closed, see Known defects) |
| `operate-review`, `operate-run-web-form` | misc | **merged** — #1022 (selected 160/6) |
| `operate-mcp` | MCP stdio server | **merged** — #1023 (`4c0236fb`) |
| `operate-poll-inbox` | CLI adapter | integrated locally — real CLI dispatch and transport replay; native trust-store behavior remains a separate gate |
| `operate-auto-confirm`, `operate-migrate` | triage / migration | **merged** — #1024 (`7d02cc58`) |

## CLI-025 execution notes

The prior "unported IMAP transport" blocker was disproved by source and
history. `crates/symeraseme-cli/src/mcp/handler.rs::poll_inbox` constructs the real
production dialer, with the source-bound nine-case handler fixture and eleven-case
transport corpus. CLI-025 now calls that handler.
`cmd/symeraseme/extra_commands.go:92-164` supplies the exact CLI contract:
only explicitly changed flags enter the argument map, `--since` and `--since-days`
share one value (last spelling wins), text output prefers a nonempty `message`,
otherwise prints `success`. Preserve these differences from other thin wrappers.
The existing `operate-poll-inbox` recording exercises a real refused local TCP
connection, not a transport-emulation string. Its native call and invalid
`--since` case now match Go; after the LLM transport slice the selected/deferred
counters are 175/0.

## CI caveat

`Rust / native (${{ matrix.os }})` reports **skipping** on PRs, so a green PR
is not native multi-platform evidence. The Rust push-to-main CI passed on
integrated `3f133e75`, including native OS jobs. This is not a cutover or
prerelease approval: the retained older Go rollback binary and release
artifact gates still require separate evidence.

## Pinned as measured, not desired

- **`plan execute --dry-run` on a web-form request appends a `SENT` event.**
  Go's `executeWebformRequest` gates only the adapter on `dry_run`, not the
  event append (the early return at `internal/campaign/execution.go:100`
  applies only when the runner is nil, and the CLI supplies one). It sends
  nothing, but it is not read-only, and the recorded `SENT` removes that
  request from the next batch. Changing it is a contract change (CLI-017).

## Remaining parity gaps

- **CLI-024 release acceptance.** The integrated native CI matrix passed on
  `3f133e75`, including the bounded registry build at 1024 descriptors
  (#1034). The disposable switchbacks build Go from current source; a
  retained older Go rollback binary reading schema v2 remains unproved
  (#1035). Do not promote this row to cutover-ready based on CI alone.
- **MCP malformed-stream breadth.** Ten source-bound Go malformed/adjacent/
  truncated process cases and ten parse/size/depth mutations match Go 1.26.6;
  128 seeded mutations replay the live Go process byte-exactly, and the new
  production-parser fuzz target completed 330,402 bounded macOS arm64
  executions historically. The current-source six-target replay and the
  configured 120-second fuzz/mutation campaign now pass (checkpoint above).
  Exhaustive or differential fuzzing is not claimed by these bounded gates.
- **`auto_confirm` with a stored reply (CLI-020).** Fails closed with an
  explicit message where Go runs `confirmation.AutoConfirm` (browser
  subsystem unported). The recorded case is the no-reply branch.
- **`go_map_order` exemption (CLI-020).** `ToolHandler::call` sorts every
  result except `auto_confirm` (Go structs keep declaration order). Any
  future Go-struct-returning tool needs the same exemption — grep the
  comment in `mcp/handler.rs`.

## Fixed parity defects (folded into the corpus or a regression test)

- **Workspace-guard edge strings.** The root-open and `InvalidInput` branches
  now match Go, with three focused Rust cases and two Go source-bound cases.
- **MCP HTTP transport and malformed-stream text.** Live HTTP starts with
  bearer auth/token rotation and signal drain; ten malformed stdio cases now
  replay Go process exit/stdout/stderr. Native platform evidence remains open.
- **`manual-tasks list` nested task key order.** Fixed by #1018
  (`a96d65d5`): `serde_json` now builds with `preserve_order`, ported
  structs keep declaration order, and every payload Go builds from a
  `map[string]any` is sorted explicitly via `jsonorder::go_map_order`.
- **`preserve_order` interaction with `plan execute` / `plan show`.**
  Found while rebasing #1018 onto the CLI-017 tree: both payloads are Go
  maps, so insertion order diverged. Fixed in the same merge
  (`execute_campaign` sorts recursively; `plan show` sorts its document).
  Lesson: any `preserve_order` consumer must classify each payload as
  struct-ordered or map-ordered against the Go source — the corpus only
  catches the cases it records.

## Decisions

- 2026-10-03 — Clean `6421dea` passed exactly 109 selected Go-free tests,
  zero failed/ignored, and the same eleven LLM tests in actual private-root
  Go 1.26.6 live mode. Native capture additionally executes the seven LLM
  fixture cases through owned loopback providers and verifies their whole
  output, raising the reviewed product-observation scope to 48. Real native
  execution remains required; the new pipeline alone proves no target.

- 2026-10-03 — #1131's actual clean `54acd9b` Go capture produced all seven
  complete LLM failure fixtures unchanged. Linux defaults verify exact
  arguments and whole raw/source/status hashes; all ten original Rust
  loopback/retry/text/redaction/15-second active-budget tests remain selected
  with one integrity control. Other OS and explicit live modes compile Go
  once into an owned temporary directory and use existing shared bounded
  build/runtime helpers. This changes test reference execution only, without
  changing production retry, TLS, provider configuration or acceptance rows.

- 2026-10-03 — Clean `5c7fd11` passed exactly 98 selected tests with Go
  absent from PATH and all eleven SQLite tests in actual private-root
  pinned-Go live mode. The next opt-in native pipeline records campaign
  execution, the actually executed Go plan-generator test/full fixture and
  SQLite snapshot/control results as well as prior families (41 product
  observations). Raw test streams and historical Python Git-blob bindings
  remain mandatory; no new native result is synthesized from Linux.

- 2026-10-03 — #1131 SQLite preparation records the actual clean `f13f405`
  Linux Go snapshot (12,585 bytes), tagged Go controls (three top-level
  tests plus four quoted-literal subcases) and complete raw status/logs.
  The Python fixture generator is verified as archived blob `28456fcf` at
  immutable `python-final`; it is absent from current source. Initial
  metadata assembly incorrectly looked for the deleted current file; the
  successful Go streams were retained and independently read back against
  that actual archived blob. All ten existing Rust tests remain selected
  with one integrity control; immutable Git/hash/schema/persisted SQL and
  co-mutated-provenance checks are unchanged. Linux defaults are frozen,
  other OS/default and explicit live paths retain bounded tagged Go.

- 2026-10-03 — Clean campaign checkpoint `65fede1` passed all 86 selected
  Go-free tests and the same six campaign tests in private-root pinned-Go
  live mode. Four actual `30eeb38` native artifacts in run `37154503080`
  independently verify 37 cases, 65 sources, eight complete runtime streams
  and installation JSON. Windows AMD64/ARM64 installation bytes are equal
  and retain their actual wrapper/mode differences. The Windows install
  default now selects those actual observations; its original Rust file,
  payload and command comparisons remain intact. The existing Windows
  POSIX-mode capability rule is unchanged. Local corpus/control validation
  is not native proof for the new frozen Rust default, which remains pending.

- 2026-10-03 — #1131 campaign preparation uses actual clean `30eeb38`
  native Linux Go execution output (6,341 bytes, nine result areas, eight
  stored events) and an actual `TestCampaignPlanBytesOracle` PASS verifying
  the unchanged full plan fixture. The raw test log, status, fixture hash
  and 86 source-file digests are retained. All four original Rust tests
  keep their full equality/source/SQL checks; two corruption controls are
  added. Other OS defaults and explicit live mode retain bounded Go. This
  prepares selected families without promoting DOM-002 or cutover status.

- 2026-10-03 — #1131's clean `b4b5a56` checkpoint passed exactly 68 selected
  tests with Go absent from PATH, six explicit live scheduler-install tests,
  and a native Linux capture of all 31 service/projection/install cases.
  The next Linux config preparation uses six actual source-bound Go cases
  (2,764 stdout bytes) from that source, retains its eleven original tests
  and adds one corruption control. Original provenance, process-output and
  timeout-cleanup checks remain selected. Mac config retains live Go and
  Windows's existing capability gate is unchanged. The native capture
  pipeline now also collects all six config cases; no all-target acceptance
  or complete Go-free workspace claim follows from these local checks.

- 2026-10-03 — #1131 preparation adds the actual native Linux Go scheduler
  installation capture at clean `7ba198d` (20 cases, 62,849 JSON bytes,
  umask 022). The first local capture under a restrictive inherited umask
  failed the original file-mode comparison and was retained separately.
  Linux defaults preserve all five existing tests and add one byte/missing
  effect corruption control; other OS defaults retain live Go until their
  full native captures exist. The opt-in native pipeline now collects all
  31 service/projection/install cases without updating committed fixtures.
  No scheduler contract/cutover status or issue is completed by this step.

- 2026-09-22 — all three wave-1 worker results came back HTTP 429 (Codex
  quota); the coordinator implemented CLI-020/021/022 itself in the slice
  worktrees. Dispatch failure is not a slice outcome.
- 2026-09-22 — `classify-reply` re-scoped out of the triage slice to
  `blocked` (llmkit transports): its recorded bytes are an `auth_failure`
  chain owned by `corekit/llmkit`, which Rust's `llm` module deliberately
  reports as not ported.
- 2026-09-22 — `ToolHandler::call` exempts `auto_confirm` from
  `go_map_order`: Go sorts map serialization but emits structs in field
  order; the recorded `confirmation.Result` bytes proved the blanket sort
  wrong.
- 2026-09-22 — CLI-021 re-scoped: `review` + `run-web-form` landed as #1022
  (`c7570432`, asserts 160/6); `operate-migrate` became CLI-023 (engine is
  907 unported lines and its single recorded case pins only the
  `validateRoots` stat error).
- 2026-09-22 — CLI-023 scoped as "port `validateRoots` for real, fail closed
  behind it": the validation walk (absolute/clean paths, symlink-component
  rejection, Go's wrapped `lstat` text) is a genuine port under the recorded
  differential; only the detection/report engine stays an explicit
  not-implemented branch. This supersedes the earlier "error-path-only would
  be emulation" verdict — emulation would be copying the error string without
  the validating code, which this is not. Landed in #1024 (`66c734af`).
- 2026-09-22 — Rust's `target_os` for Go's `runtime.GOOS == "darwin"` is
  spelled `macos`; a `#[cfg(target_os = "darwin")]` allow-list compiles
  clean and silently never fires (found via the recorded `/tmp` symlink
  component). Go platform guards port as `cfg(target_os = "macos")`.
- 2026-09-22 — Rust CI gates a 90 % line coverage on `mcp/**`-matching
  files (`rust-ci.yml` critical set); subprocess corpus replays are not
  llvm-cov-instrumented, so new handler/serve code needs in-process tests.
  Local replication: `cargo llvm-cov --workspace --all-features --json`
  plus the workflow's file filter — #1023 failed at 89.65 % and passed at
  90.43 % after its stream tests.
- 2026-09-22 — the differential replay now substitutes the recorded
  `<ORACLE_ROOT>` in argv and folds the runtime root back out of stdout and
  stderr (byte-level), restoring the capture's normalization; before this,
  no path-bearing case could replay byte-exactly, in Go or Rust.
- 2026-09-22 — a failed dispatch on HTTP 429 (Codex quota) is not a slice
  outcome; the coordinator implemented CLI-021 in its worktree, per the
  dispatch contract.
- 2026-09-22 — wave 1 of this run dispatched three writers in parallel
  (CLI-020/021/022) with disjoint subsystem scopes but a known shared-edit
  surface (`cli.rs` dispatch arms + `command_surface.rs` selection/counts);
  the coordinator merges serially and owns the final count reconciliation
  (target selected 163 / deferred 3 — the third deferred slot after the
  three blocked llmkit/IMAP cases is `classify-reply`, also llmkit).
- 2026-09-21 — this ledger created; `docs/rust-port/handoffs/` stays the
  per-slice evidence store, not a second tracker.
- 2026-09-21 — CLI-017 merged as #1019 (`c0573f86`).
- 2026-09-21 — CLI-015 merged as #1016 (`cedc0b8a`); CLI-016 as #1017 (`04e516b3`).
- 2026-09-21 — key-order fix merged as #1018 (`a96d65d5`); CLI-018
  (events/grant) merged as #1020 (`861527cf`). Selected 155 / deferred 11.
- 2026-09-21 — CLI-019 (generate-dashboard/-report/-scheduler) merged as
  #1021 (`93872f48`). Selected 158 / deferred 8. `operate-generate-rebuttal`
  stays deferred: the LLM subsystem is unported and its auth-failure string
  must not be emulated. Delegated worker for this slice died on Codex 429
  (fallback did not trigger); the coordinator implemented the slice directly
  in the slice worktree.
- Port PRs carry code only; ledger updates go to main separately.
- Work is dispatched into per-slice worktrees off the integrated revision; the
  coordinator alone edits this file, shared manifests and CI.

## Out of authorization

Go deletion, release publication and production cutover. The Go tree stays
runnable — it is the oracle.
