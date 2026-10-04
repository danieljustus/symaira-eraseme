# Email transport crate decision

Decision for tasks 6.1, DOM-006 and DOM-007, 2026-10-03. This records the
implemented choices and six-target native trust acceptance under #1119.

| Boundary | Selected implementation | Reason and executable contract |
| --- | --- | --- |
| IMAP framing, UID/HWM and XOAUTH2 | Existing bounded Rust adapter in `email/imap` | Keeps the recorded Go command/tag, literal and SASL transcripts. `imap_transport_parity` exercises implicit TLS and STARTTLS; `email_parity` checks mailbox policy and high-water marks. Replacing this adapter with a different IMAP crate is not needed to complete the existing contract. |
| TLS | Existing pinned `rustls` and `rustls-native-certs`; Windows `schannel` root enumeration | Loads the OS certificates, including valid server-auth roots in Windows LocalMachine ROOT. Explicit `SSL_CERT_FILE`/`SSL_CERT_DIR` roots retain their existing precedence. Shared `email/tls.rs` now supplies both IMAP and OAuth2. |
| OAuth2 HTTPS | Existing pinned `ureq`, with the shared platform certificate set | Go uses system trust. The default ureq Mozilla roots rejected a private configured CA in a real TLS regression. Supplying the platform roots repairs that difference without adding a dependency. Certificate verification, bounded timeouts, no redirects and provider-body redaction remain enabled. |
| MIME / SMTP boundary | Existing deterministic MIME builder and `SmtpTransport` trait | `smtp_parity` pins Go MIME bytes, recipient order and error text. An actual SMTP network adapter and campaign sender acceptance remain a separate #1118 prerequisite; this decision does not claim that adapter exists. |

The new OAuth2 regression uses three distinct private certificate cases:
trusted issuer, unrelated issuer and an expired server certificate. The trusted
case failed before the fix. Afterward it passes, while both negative cases
reject the handshake before any OAuth2 form is delivered. Existing seven
OAuth2 and five IMAP parity tests pass locally. No mailbox, provider, operator
credential or paid service is used.

`email-native-trust.yml` runs the explicitly selected native test on all six
GitHub-hosted OS/architecture targets. That test installs its uniquely named
private CA in the real OS store, removes certificate-file overrides in the
child, checks OAuth2 HTTPS plus IMAP TLS/STARTTLS for all three certificate
classes, and removes its own CA and trust entry. It refuses to modify trust
outside a disposable GitHub-hosted runner. A normal local test run ignores
this privileged control; that ignore is not native evidence.

Retained cleanup failure: native run `37133611213` at
`ff9f4c67606d65868d4dee8de28a3274e8070999` passes all twelve TLS cases
on both Linux and both Windows targets. macOS ARM job `111233501324`
passes the nine pre-cleanup cases, then reports that the newer native admin
store has no owned certificate while the classic external representation
still has its exact SHA-1 entry. This is failed cleanup acceptance.

The accepted macOS fixture first removes a measured native-store entry if present.
If the classic entry remains, it considers only Apple's two fixed admin-plist
locations. Exactly one readable regular, single-link file must equal the
complete native external representation. Under an exclusive lock, inode,
owner/group, mode and original bytes must still match; it removes only the
uniquely generated certificate digest, fsyncs, and verifies complete remaining
settings and unchanged ownership/permissions. No vault/authorization permission
is relaxed, no directory is searched, and unreadable or ambiguous stores fail
closed. Native external readback and deletion of the uniquely named Keychain
certificate remain required. Fresh-process OAuth2, IMAP TLS and STARTTLS must
then reject the formerly trusted CA without delivering credential bytes.
This fixture-only legacy cleanup route has actual proof on both native Mac
architectures in the accepted run below.

## Six-target native acceptance

At branch source `d35ddca04abebe03e8a90fb77be97fb4129f7f74`,
[run 37137506583](https://github.com/danieljustus/symaira-eraseme/actions/runs/37137506583)
passed Linux amd64/arm64 (`111244924095`/`111244923952`), Windows amd64/arm64
(`111244924158`/`111244924149`) and macOS amd64/arm64
(`111244924117`/`111244924197`). The actual clean PR merge source is
`9c2b43626d4994a59ff58b66fee1e9e5c00fced4`.

Every native job executes the three command lifecycle controls (success,
actual exit 23 and bounded stalled-child termination), installs the unique CA
in the real OS store with certificate-file overrides absent, and checks twelve
TLS cases: trusted, unrelated, expired and fresh-process formerly-trusted
after removal, each through OAuth2 HTTPS, IMAP TLS and STARTTLS. Both native
Mac logs record successful bounded exact-entry cleanup, deletion of the owned
Keychain certificate and all three formerly accepted connections rejected in
a new process. Untrusted/expired/removed cases deliver no credential/form
bytes. Ten mailbox, five IMAP transport and seven OAuth2 transcript tests also
pass in every job; local privileged skips are not counted as native evidence.

DOM-006/DOM-007 and task 6.1's crate-decision evidence now meet their native
scope. Tasks 6.1/6.3/6.4 still await final PR checks and actual main integration.
No dependency, certificate-verification policy or timeout was relaxed.

Earlier failures remain retained in their run logs:

| Source/run | Measured failure |
| --- | --- |
| `fa21054f` / `37114908180` | macOS passed nine TLS cases, then the unbounded native cleanup hit the workflow deadline. |
| `6a9a04de` / `37118725509` | Bounded `security remove-trusted-cert` exceeded thirty seconds. |
| `f2554fc2` / `37120643595` | Temporary authorization restoration passed; root-session removal still stalled. |
| `8319356d` / `37121189734` | Login-session removal also stalled; authorization restoration passed. |
| `2d661295` / `37122245131` | Export ownership prevented the runner reading its root-owned private plist. |
| `e61f49ac` / `37122888055` | Native authorization write/restoration returned `NO (-60005)` before CA installation. The override was removed. |
| `2a514352` / `37126123832` | Both macOS targets passed nine TLS cases; direct admin import exceeded thirty seconds. |
| `1a5b936` / `37130671704` | User-domain installation exceeded thirty seconds on macOS ARM before TLS cases. |
| `ff9f4c67` / `37133611213` | macOS ARM classic entry exists, but native private admin-store membership is false. |

Do not infer native acceptance from local certificate-file tests, builds or
queued CI. The accepted run above contains both macOS cleanup results,
all twelve actual OS-store TLS cases and source-bound mailbox/OAuth2
transcript gates remain mandatory. No mailbox, operator profile or paid
provider is used. The finite probes do not establish every chain-policy case.

## Verified main integration (2026-10-04)

Exact head `b07e81d02bcd35d424c5c75090992f1d1b8fe3c1` passed six native
trust jobs in run `37197122235`, all six full workspace jobs in `37199861723`
and all PR checks. PR #1146 merged normally by squash as
`ee03eb277ade28fd6deaef976f7d9bfc4db8a468`; its Git tree equals the tested
head. #1119 is closed/completed, so tasks 6.1/6.3/6.4 are integrated. Earlier
pending statements describe the earlier source and are superseded here.
