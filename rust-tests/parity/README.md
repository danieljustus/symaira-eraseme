# Symaira EraseMe Parity Test Suite

This directory contains the differential testing harness, frozen baselines, and
oracle fixtures used to guarantee 100% behavioral, binary, and protocol parity
during the Go-to-Rust migration (see
[docs/plans/2026-09-04-go-to-rust-migration-proposal.md](../../docs/plans/2026-09-04-go-to-rust-migration-proposal.md)
and [docs/rust-port-contract-matrix.md](../../docs/rust-port-contract-matrix.md)).

## 1. Structure

### Frozen neutral and cryptography suite preparation (#1131)

`make parity` now tests the neutral harness without building Go. Its time and
confirmation differential loads actual Go 1.26.6 observations from
`fixtures/frozen/go1.26.6/time-confirmation/`, including capture provenance,
full-output SHA-256 and input SHA-256 checks. All 32 timestamp and eight ordered
URL cases still run. Three corrupted-observation controls must fail: changed
ISO output, a missing timestamp row and an added URL. The original harness
mutation/process/filesystem/HTTP/SQLite checks remain selected.

`make parity-live` explicitly rebuilds Go and selects the same cases with
`SYMERASEME_PARITY_LIVE_GO=1`. Live observation requires Go 1.26.6 and uses
private HOME/USERPROFILE/XDG/temp/data roots, regular capture files and child
cleanup. Cache discovery, build and execution share the existing total
30-second budget; the deadline is not increased. Default tests require no
Go executable. The Go-free CI job checks this with a PATH containing no Go
compiler executable and offline Rust dependency resolution after fetch.

The core `identity_interop` and `encryption_parity` integration tests also
default to actual recorded Go observations. The 15 complete requests, stdout,
stderr and statuses in `tests/fixtures/go-frozen/identity-crypto/` include five
rejected tampered envelopes and actual Rust-writer envelopes consumed by Go.
The manifest pins the clean capture source, native Go version and every byte
stream's length and SHA-256. Lookup requires an exact recorded request; an
unknown ciphertext cannot receive a cached plaintext. All 17 encryption and
four original identity tests remain selected, with an additional corpus
control checking all 15 records, the five Rust rejections and an unknown key.

Two private writer tests reproduce the complete observed Go/Rust envelopes
using existing private nonce/salt/IV/clock helpers. Public encryption still
uses fresh production randomness, and fresh-envelope round trips remain
checked. Explicit `SYMERASEME_PARITY_LIVE_GO=1` retains live interoperability
for these integrations. The Go-free workflow executes both integration files
and the two writer tests as well as the neutral package; other core tests
remain outside this preparation's Go-free claim.

The core `triage_service` integration also defaults to the actual complete
Go classify/rebuttal/fallback/error output in
`tests/fixtures/go-frozen/triage-service/`. It verifies the captured raw
length/digest and retains all original source hashes and persisted-effect
comparisons. Five original tests and a byte-change/missing-operation control
remain selected; explicit live mode runs the original Go service oracle.

To collect new service and architecture-specific projection observations,
dispatch existing Rust CI with `capture_frozen_oracles: true`. Its six native
jobs run `scripts/capture-frozen-go-services.py` and upload immutable raw
outputs with source/toolchain/native-target provenance. This capture does
not overwrite fixtures or synthesize an unobserved target result. The regular
native Rust suite still executes; capture success alone is not suite proof.

The core `projection_oversized_parity` integration defaults to actual separate
AMD64/ARM64 captures in `tests/fixtures/go-frozen/projection/`. Four native
Linux/Windows artifacts verify seven cases and show exactly one architecture
difference (`int64_max_payload`). Every original Rust comparison remains
selected, with two added byte-change/missing-case controls in one test.
Source/input pins and complete output digests guard both observed corpora.
The Go-free workflow selects this integration too; Mac capture and complete
corrected native suite acceptance still remain pending.

Captured streams and raw neutral observations use explicit Git byte-preserving
attributes; input JSON uses LF. This retains the strict hashes on Windows.
The original 6ab9537 Windows CRLF failures and actual Git/Rust negative control
remain documented rather than normalized away.

The engine `scheduler_parity` integration likewise defaults to actual
source-bound Go output in `tests/fixtures/go-frozen/scheduler/`, preserving
six generator configurations, twelve legacy inputs and all six original
Rust tests. An added byte-change/missing-case control checks integrity.
Explicit live mode retains the real Go comparison through the shared
bounded runner. Scheduler install/config and the complete engine suite
are not covered by this family's Go-free claim.

The `scheduler_install_parity` integration now defaults on Linux to a real
Go 1.26.6 twenty-case capture at clean `7ba198d`, produced with the fixture's
umask 022. All original file hashes, modes, errors and recorded commands are
compared. The five existing tests plus a byte-change/missing-file control
execute without Go on Linux. Mac retains live Go until its actual native
captures are reviewed; no Unix fixture is presented as a
Windows observation. Explicit live mode uses the existing bounded oracle
runner. Native capture now records these twenty cases in addition to the
eleven service/projection observations, including the complete JSON file.

The Linux `config_parity` default also reads an actual six-case Go 1.26.6
capture from clean `b4b5a56`. All eleven original tests remain selected,
including subprocess output/timeout cleanup controls; an added corruption
control rejects changed raw bytes and a missing configuration case. The
existing provenance at `119ee9f` and original six Rust configuration
comparisons remain unchanged. Mac retains live Go and Windows retains its
original capability gate. Native capture now additionally collects these
six configuration observations (37 total service/projection/install/config
cases). These Linux family additions do not make the whole workspace Go-free.

Linux campaign plan/execution tests also use real observations from clean
`30eeb38`. The complete 6,341-byte execution document retains all nine result
areas and eight stored events. The actual pinned-Go plan generator test ran
once and verified the existing complete byte fixture unchanged; its raw
JSON test log/status, fixture digest and 86 source-file hashes are recorded.
The four existing Rust tests remain selected with two corruption controls.
The original source pins, whole-byte plan/effect comparisons and complete
execution equality are retained. Other OS defaults and explicit live mode
run bounded Go; no native campaign acceptance is inferred from Linux data.

The scheduler-install default now also uses the actual Windows document from
run `37154503080` at clean `30eeb38`. Both native Windows architecture
artifacts contain byte-identical twenty-case documents; their unchanged raw
manifests and the four-target ZIP/source/stream readback are retained.
Windows-specific wrapper hashes and observed Go permission values remain
intact, and all original Rust payload/file/command comparisons are preserved.
The existing Windows capability rule for POSIX mode comparison is unchanged.
A native-corpus control verifies both observed architectures and rejects an
unknown target or changed bytes. Local readback is not native execution of
the new frozen Windows Rust path; that acceptance is still pending.

Linux SQLite contract tests now read the real source-bound Go snapshot at
clean `f13f405`, containing complete fresh/golden schema, pragmas, row counts
and immutable provenance. The actual tagged Go test run includes three
top-level controls and four quoted-literal subcases; its raw status/logs are
verified. All ten existing Rust tests remain selected, including altered
and co-altered fixture/provenance rejections and immutable Git-blob checks,
plus a byte-change/missing-schema control. The Go-free checkout fetches full
history for those unchanged fail-closed checks. Other OS defaults and
explicit live mode retain the existing bounded tagged Go runner.

The next opt-in native capture includes campaign execution, the actually
executed plan-byte generator and tagged SQLite snapshot/control tests as
well as the existing service/projection/install/config families. It records
41 product observations, full plan/install documents and raw Go-control
logs only after actual success. The original historical Python generator
is bound as an archived Git blob, not a deleted current source file. This
pipeline extension requires real native execution before those new target
observations can be accepted; it never updates committed fixtures.

The Linux LLM failure family also defaults to seven actual whole Go
observations from clean `54acd9b`. Argument lookup is exact and every raw
fixture/source/status hash remains strict. All ten original tests still run
their real Rust loopback requests, retry/text/redaction comparisons and the
unchanged 15-second active-fixture controls; one corruption/unobserved-args
test is added. Other OS defaults and explicit live mode build the same Go
oracle once into an owned temporary directory, use the existing shared
120-second build/30-second execution limits, and retain all seven byte
comparisons. No production retry/TLS/config or provider traffic is changed.
The next native capture also runs these seven fixed LLM cases against owned
loopback providers and verifies their complete output against the same
fixtures. Together with prior families, it records 48 product observations;
native acceptance still requires actual target execution and review.

This is preparation for #1131. Other core/CLI/engine runtime oracles, the
command-line harness's live `--go` interface, and switchback runners are still
to be frozen. The suite-wide Go retirement, release prerequisites and actual
seven-day observation remain open. No live-mode observation is fabricated,
and no production randomness or Rust/Go comparator is relaxed.

```
rust-tests/parity/
├── README.md               # This document: harness overview and baseline definitions
├── baselines/              # Frozen release baselines and cutover performance targets
│   └── v0.12.1.json        # Pinned Go baseline from tag v0.12.1 (commit 240bf67c...)
├── cases/                  # Parity test cases across CLI, MCP, HTTP, and DB (Task 0.4+)
└── fixtures/               # Pinned input/output fixtures and golden files (Task 0.4+)
```

## 2. Frozen Release Baseline (`baselines/v0.12.1.json`)

The baseline file captures the exact physical, coverage, performance, and
release asset state of Symaira EraseMe at `v0.12.1` (commit
`240bf67cefa05e643e32611a02e6e7ed87a033ea`).

### Key Metrics Summary

| Metric | Frozen Go Baseline (`v0.12.1`) | Cutover Gate / Requirement |
|---|---:|---|
| **Git Commit** | `240bf67cefa05e643e32611a02e6e7ed87a033ea` | Exact tag resolution |
| **Go Source Files** | 122 tracked `.go` files | Full parity across all modules |
| **Physical Source Lines** | 23,179 lines | Clean exported tree scope |
| **Embedded Brokers** | 1,277 validated brokers | All 1,279 YAMLs (minus 2 example docs) |
| **MCP Catalogue** | 26 pinned tools | `internal/mcp/tools.json` schema v1 |
| **Go Statement Coverage** | 76.23% (5,225/6,854 statements) | Must exceed 75% gate |
| **Deterministic arm64 Binary** | 16,758,114 bytes (`7849d247...`) | `GOOS=darwin GOARCH=arm64 CGO_ENABLED=0 go build -trimpath -buildvcs=false -ldflags "-s -w"` |
| **Released arm64 Binary** | 16,686,738 bytes | Unpacked from `symeraseme_0.12.1_darwin_arm64.tar.gz` |
| **Startup Latency (100 runs)** | median 9.30 ms, p95 9.97 ms | No regression > 20% (p95 ≤ 11.96 ms) |
| **Maximum RSS** | 22,282,240 bytes (`/usr/bin/time -l`) | No regression > 20% (RSS ≤ 26,738,688 bytes) |
| **Release Archive Targets** | 6 CLI archives + 1 macOS DMG | Exact filenames, checksums, root layout, and manifest |

### Official v0.12.1 Release Archives

All six cross-platform release archives and companion assets are fetched and
verified dynamically from GitHub releases at capture/verify time. The harness strictly
asserts the exact target set `darwin/linux/windows × amd64/arm64`, the exact DMG filename
(`Symaira-EraseMe-0.12.1-macos.dmg`), and the archive root layout (`symeraseme` in `.tar.gz`,
`symeraseme.exe` in `.zip`):

1. `symeraseme_0.12.1_darwin_amd64.tar.gz` (6,837,118 bytes) — `3ff650cc1cab17e23f1c7264006b21b43d5e23a67ee6783daba84e39357869a1`
2. `symeraseme_0.12.1_darwin_arm64.tar.gz` (6,472,923 bytes) — `7fa696829c9bf861ba902a65576d22013e4eeeb655150143975f961078dc906b`
3. `symeraseme_0.12.1_linux_amd64.tar.gz` (6,895,950 bytes) — `f64bc2d8456f3e9b7e66763e4f90dc42b43c526df61593e228676a2663d43d15`
4. `symeraseme_0.12.1_linux_arm64.tar.gz` (6,386,000 bytes) — `02613a59bd88657c436ee8d748f33cc0910ec0cafb07cd238f3e1fd520e74dfd`
5. `symeraseme_0.12.1_windows_amd64.zip` (6,907,665 bytes) — `a4ff2c47c9ff1bc7d9e4becdf390dad398e3fe059ceab598e7e7bff1e1a4e54f`
6. `symeraseme_0.12.1_windows_arm64.zip` (6,296,129 bytes) — `d2771bc8d8c68e636b5df2a82bf29ba583e38c26f56bf87af2c86ec825412d9b`
7. `Symaira-EraseMe-0.12.1-macos.dmg` (7,259,990 bytes) — `c0d024c3b1063d14eec39abfc69f14a72ff7e5a4a885bb8fa901d56a68339baf`
8. `checksums.txt` (717 bytes) — `a3e7b606ff4f380bf324a81987754d28b78daa2d714d2a22419109954b8e33ad`

### Checksums Format

The release uses the canonical `sha256sum` format: `<64-hex-digest><two spaces><filename><newline>`.
All downloaded archive and disk image digests are verified against `checksums.txt`.

### DMG Bundle Manifest

On macOS, `Symaira-EraseMe-0.12.1-macos.dmg` is dynamically mounted (`hdiutil attach -nobrowse -readonly`)
and inspected to verify its volume name (`Symaira EraseMe`), filesystem format (`Apple_HFS / GUID_partition_scheme`),
and bundle manifest:
- `Applications` (symlink to `/Applications`)
- `.background/symaira-dmg-background.png` (installer backdrop)
- `Symaira EraseMe.app/Contents/Info.plist`
- `Symaira EraseMe.app/Contents/MacOS/Symaira EraseMe` (SwiftUI frontend)
- `Symaira EraseMe.app/Contents/MacOS/symeraseme` (embedded helper binary)
- `Symaira EraseMe.app/Contents/Resources/AppIcon.icns`
- `Symaira EraseMe.app/Contents/_CodeSignature/CodeResources`

On non-macOS platforms, DMG filesystem inspection is honestly reported as unsupported in `dynamic_metadata.dmg_inspection`.

## 3. Stable Fields vs. Dynamic Metadata

To ensure deterministic verification in CI and local testing, `v0.12.1.json` is
split into two top-level sections:

- **`stable_fields`**: All deterministic properties of the repository, source
  code, test coverage, build artifacts, frozen performance targets, official release
  assets, DMG manifest, and target architectures (`os: darwin`, `arch: arm64`).
  All source metrics, test coverage, and binary compilation are performed on an
  isolated export of tag `v0.12.1` (`git archive`), guaranteeing isolation from
  uncommitted working tree state. Running `scripts/capture-go-baseline.sh` repeatedly
  guarantees that `jq .stable_fields` produces bitwise-identical output.
- **`dynamic_metadata`**: Execution-specific telemetry, including capture
  timestamp (`captured_at`), host runner toolchain versions (`go_version`,
  `git_version`, `swift_version`), DMG inspection status, and empirical 100-run
  benchmark measurements.

## 4. Capture & Verification Script

The capture script is located at `scripts/capture-go-baseline.sh`.

### Usage

```bash
# Capture baseline (including 100-run benchmark on Darwin arm64) and write to rust-tests/parity/baselines/v0.12.1.json:
./scripts/capture-go-baseline.sh

# Verify that current repo state matches existing baseline stable fields (skips 100-run benchmark by default):
./scripts/capture-go-baseline.sh --verify

# Verify including the 100-run startup benchmark (fails if runner cannot execute darwin/arm64):
./scripts/capture-go-baseline.sh --verify --with-benchmark

# Output to a custom path:
./scripts/capture-go-baseline.sh --output /path/to/output.json

# Capture skipping benchmark:
./scripts/capture-go-baseline.sh --skip-benchmark
```

### Benchmark Execution Policy

The 100-run startup benchmark executes only when the host runner can execute `darwin/arm64`
(macOS Darwin arm64). When running on other architectures (e.g. Linux or Intel macOS), the
benchmark is honestly skipped in `dynamic_metadata.live_100_run_startup` unless `--with-benchmark`
was explicitly requested, in which case it fails immediately with a descriptive error.

### Privacy Guarantee

`scripts/capture-go-baseline.sh` runs with strict sandboxing:
- Isolates all source extraction, compilation, and asset downloads in a temporary directory.
- Overrides `SYMERASEME_DATA_DIR`, `SYMERASEME_DB_DIR`, `SYMERASEME_CONFIG_DIR`, and `HOME`
  to isolated sandbox directories.
- Clears credentials such as `ANTHROPIC_API_KEY`.
- Never reads or records personal directories (`~/.symeraseme`, `~/.config/symeraseme`),
  user tokens, or usernames.
- Redacts machine identifiers from baseline records.
