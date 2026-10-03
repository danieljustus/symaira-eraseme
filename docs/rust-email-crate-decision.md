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

At source `fa21054f`, native run `37114908180` passed the complete controls on
both Linux and both Windows targets. Intel macOS job `111179740236` completed
all three certificate classes for all three transports, then hit the workflow
deadline during CA cleanup. This is a failed acceptance gate, not PASS evidence.
The command helper now records bounded native command stages into regular
files, rather than waiting for pipe EOF, and enforces a 30-second command
lifetime. Run `37118725509` at `6a9a04de` localizes the actual macOS stall to
`security remove-trusted-cert`; import and all TLS cases finish first. The
disposable macOS fixture now saves the `com.apple.trust-settings.admin` rule,
temporarily allows its noninteractive trust operations, then restores that
rule from its original plist and verifies all policy fields (excluding only
generated timestamps/version). Successful CA removal and verified rule
restoration are required before acceptance. Production certificate policy is
unchanged; final macOS cleanup and six-target acceptance remain pending.

Run `37120643595` at `f2554fc2` confirms that allowing and then restoring the
admin trust rule succeeds, but sudo/root-session removal still stalls. The
next native probe executes the owned admin trust removal in the runner's
login session with that temporary authorization, retaining the same 30-second
bound and mandatory restoration. This is a pending cleanup probe, not an
established root cause or completed acceptance.

Do not promote DOM-006/DOM-007 or task 6.1 solely from this document, a local
certificate-file test, a successful build, or queued CI. Acceptance requires
completed native results and the source-bound Go transcript gates. The finite
TLS controls do not establish every platform chain-policy corner case.
