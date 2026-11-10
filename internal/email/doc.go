// Package email contains the Go email boundary for symaira-eraseme.
//
// SMTP and IMAP are implemented behind small interfaces so all network and
// credential operations can be replaced in deterministic tests. OAuth2 token
// exchange is also an explicit HTTP boundary; token values are never logged or
// included in returned errors. Live mailbox delivery/authentication remains an
// external verification step and is intentionally not performed by unit tests.
//
// Himalaya is deliberately not ported into this package. The Python CLI
// retains its optional Himalaya subprocess backend for compatibility, while the
// Go port uses SMTP/IMAP directly. This avoids making a CLI subprocess part of
// the Go secret boundary and keeps the Go package standalone.
package email
