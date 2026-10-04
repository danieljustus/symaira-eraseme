# Native SMTP campaign acceptance

`NetSmtpTransport` implements the explicit outbound adapter from Go's
`NetSMTPTransport`: EHLO/HELO, STARTTLS, password or XOAUTH2 authentication,
MAIL/RCPT/DATA, dot framing and QUIT. It uses the existing pinned Rust TLS and
OS-root primitives; no dependency or message-building crate is added. The
existing deterministic MIME implementation remains the byte contract.
`SmtpConfig` contains operational inputs and has a redacted Debug formatter.
Connection/read/write waits use the configured timeout or 30 seconds by
default; SMTP replies have finite line and aggregate limits. These bounds do
not claim a total wall-clock deadline for an entire multi-recipient operation.

The campaign only sends when its caller supplies the email adapter. The CLI
and MCP's current no-sender path keeps the same Go-compatible error behavior.
Default SMTP configuration requires STARTTLS; cleartext password auth follows
Go's exact localhost names. Certificate verification stays enabled, with TLS
1.2 or newer and an explicit private-CA configuration when supplied.

`campaign_smtp` executes the actual Go sender and Rust sender against two local
synthetic SMTP transactions: rejected recipient, then accepted message. It
compares raw transaction/MIME bytes (folding only Go's live Date header),
results, complete persisted event payloads/sources and the complete projected
GetPlan after the same explicit timestamp pinning used by the retained
campaign oracle. No other wire field, error, token or message ID is normalized.
Six implementation/helper hashes and Go 1.26.6 are verified.

`smtp_transport` compares eight additional actual Go/Rust transactions for
password and OAuth2 auth, auth rejection/challenge, absent STARTTLS, rejected
DATA/greeting and HELO fallback. It includes To/Cc/Bcc envelope order and dot,
LF, CRLF and terminal-CR framing. Three real Rust STARTTLS handshakes check a
private trusted chain, unrelated issuer and expired leaf; both negative cases
must deliver no application/auth bytes. These injected private-root controls
are distinct from #1119's native OS-store trust acceptance.

One additional server-echo negative control records Go's real diagnostic
containing the synthetic password and encoded PLAIN payload. The Rust adapter
keeps the same authentication/abort transaction while removing raw, quoted
and encoded credential spellings from its error. This deliberate redaction
enforces the transport's privacy contract; that diagnostic is not presented as
a byte-equal Go error. All ordinary error controls remain byte-equal.

Local Linux execution passes these controls and the three existing MIME tests,
strict workspace Clippy, formatting and Go vet. The six-target workflow also
selects the retained plan/Go execution suites and actual consented plan CLI
process pairs. At branch source
`cb3234c2291c8b243aeed8452e2bfce6f1938868`,
[run 37137509483](https://github.com/danieljustus/symaira-eraseme/actions/runs/37137509483)
passed all six native targets: Linux amd64/arm64 jobs
`111244931322`/`111244931484`, Windows amd64/arm64
`111244931505`/`111244931531`, and macOS amd64/arm64
`111244931566`/`111244931454`. Each executes the actual campaign SMTP pair,
eight ordinary protocol comparisons plus the credential-echo privacy control,
all three private-root TLS cases, MIME, plan/execution and retained CLI gates.
DOM-002/CLI-010 now meet that native scope. Required final PR checks and actual
main integration remain separate. These synthetic local transactions do not
claim paid-provider execution or the independent #1119 OS-store trust gate.
