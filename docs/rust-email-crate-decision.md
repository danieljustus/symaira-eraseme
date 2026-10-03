# Email transport crate decision

Decision for tasks 6.1, DOM-006 and DOM-007, 2026-10-03. This records the
implemented choices; native trust acceptance remains tracked by #1119.

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

Current cleanup probe: native run `37133611213` at
`ff9f4c67606d65868d4dee8de28a3274e8070999` passes all twelve TLS cases
on both Linux and both Windows targets. macOS ARM job `111233501324`
passes the nine pre-cleanup cases, then reports that the newer native admin
store has no owned certificate while the classic external representation
still has its exact SHA-1 entry. This is failed cleanup acceptance.

The next macOS fixture first removes a measured native-store entry if present.
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
This fixture-only legacy cleanup route still requires actual native proof.

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

Do not promote DOM-006/DOM-007 or tasks 6.1/6.3/6.4 from local
certificate-file tests, builds or queued CI. Both macOS cleanup results,
all twelve actual OS-store TLS cases and source-bound mailbox/OAuth2
transcript gates remain mandatory. No mailbox, operator profile or paid
provider is used. The finite probes do not establish every chain-policy case.
