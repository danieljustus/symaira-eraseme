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

Run `37121189734` at `8319356d` also stalls in login-session removal,
while rule restoration again passes. The next fixture uses the external
admin trust representation API: export the current settings, remove exactly
the owned certificate SHA-1 entry, import, re-export and compare the complete
remaining settings before deleting its unique Keychain certificate. Every
command retains the 30-second bound. No unrelated trust entry may change;
this cleanup route still requires native validation.

The first external-representation probe (`37122245131`, `2d661295`)
exports successfully, then fails because sudo creates a root-owned 0600
plist that the runner cannot read. Exports now run as the runner; only
the actual admin import remains privileged. This preserves private file
ownership and the complete remaining-trust comparison. Native import/removal
acceptance is still pending.

The current `e61f49ac` probe `37122888055` failed on macOS Intel job
`111202340118` before installing the CA: `security authorizationdb write`
returned `NO (-60005)` for both temporary override and attempted restoration.
The speculative authorization override did not repair the earlier removal
hangs and is now removed entirely. The existing hosted-runner `sudo -n`
certificate installation remains; cleanup directly imports the trust
representation with exactly the owned entry removed, re-exports to verify all
unrelated settings and then deletes the owned certificate. Native results for
this direct route are pending. No authorization database is modified by the
current candidate; Linux/Windows production trust logic is unchanged.

Direct admin import at `2a514352` in run `37126123832` also exceeds the
30-second bound on macOS ARM job `111211644787` and Intel job `111211644710`,
after all nine native TLS controls pass. Exporting/removing only the owned
entry succeeds; the privileged import stalls. No cleanup success is claimed.

The next candidate uses the disposable hosted runner's existing default user
keychain and native user trust domain. Apple trustd distinguishes the user
and admin authorization rights; no authorization database override is used.
The fixture requires the keychain to be an existing absolute file under the
runner's HOME, imports only the uniquely generated CA, and removes its trust
and certificate with user-domain native APIs. A Security.framework read then
compares complete remaining trust settings. Native errSecNoTrustSettings is
accepted only when the expected unrelated-entry set is empty. All platforms
add a fresh-process post-cleanup control that requires the formerly trusted
CA to fail OAuth2 HTTPS, IMAP TLS and STARTTLS without transmitting credentials.
This route and cleanup control require actual native validation; the existing
failed probes remain retained and the matrix stays PARTIAL.
