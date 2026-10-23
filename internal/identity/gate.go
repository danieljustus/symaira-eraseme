// Package identity — gate.go: the top-level consent gate and small
// helpers (random URL-safe token, TTY prompt).  Mirrors
// check_consent in src/symeraseme/core/consent.py.
package identity

import (
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// ErrConsentDenied is returned by ConsentGate when the user did not
// grant consent through any of the available channels.
var ErrConsentDenied = errors.New("identity: consent denied")

// RandomURLSafe returns a url-safe random string of n bytes (the
// resulting string is base64.RawURLEncoding of n random bytes).
// Mirrors secrets.token_urlsafe(16) used by issue_token.
func RandomURLSafe(n int) string {
	if n <= 0 {
		n = 16
	}
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic(fmt.Sprintf("identity: rand.Read failed: %v", err))
	}
	return base64.RawURLEncoding.EncodeToString(b)
}

// TTYAvailable reports whether stdin/stdout are TTYs.  Mirrors
// tty_available() in the Python module.
func TTYAvailable() bool {
	fi, err := os.Stdin.Stat()
	if err != nil {
		return false
	}
	if (fi.Mode() & os.ModeCharDevice) == 0 {
		return false
	}
	fo, err := os.Stdout.Stat()
	if err != nil {
		return false
	}
	return (fo.Mode() & os.ModeCharDevice) != 0
}

// TTYPrompt asks the user on the terminal and returns true on an
// affirmative answer.  Returns false when no TTY is available or
// the user dismisses the prompt.
func TTYPrompt(message string) bool {
	if !TTYAvailable() {
		return false
	}
	fmt.Fprintf(os.Stderr, "%s [y/N] ", message)
	var response string
	if _, err := fmt.Scanln(&response); err != nil {
		return false
	}
	switch strings.ToLower(strings.TrimSpace(response)) {
	case "y", "yes":
		return true
	default:
		return false
	}
}

// ReadConsentFile reads a consent token from a file with
// permission and sanity checks.  Mirrors _read_consent_file.
func ReadConsentFile(path string) (string, error) {
	if path == "" {
		return "", nil
	}
	p, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	data, err := os.ReadFile(p) //nolint:gosec // path supplied by caller
	if err != nil {
		return "", err
	}
	st, err := os.Stat(p)
	if err != nil {
		return "", err
	}
	if !st.Mode().IsRegular() && p != "/dev/stdin" && p != "/dev/fd/0" {
		return "", fmt.Errorf("identity: consent file is not a regular file")
	}
	if st.Mode().Perm() != ProfileFileMode {
		_ = os.Chmod(p, ProfileFileMode)
	}
	text := strings.TrimSpace(string(data))
	if text == "" {
		return "", fmt.Errorf("identity: consent file is empty")
	}
	if idx := strings.IndexByte(text, '\n'); idx >= 0 {
		text = text[:idx]
	}
	return strings.TrimSpace(text), nil
}

// ConsentGate evaluates the precedence chain for destructive
// commands: --yes > --consent token > --consent-file > env-var
// token > env-var file > interactive TTY prompt.
//
// On success the token (when used) is consumed so the next
// destructive command requires a fresh grant.  The returned error
// is ErrConsentDenied when nothing in the chain grants consent.
func ConsentGate(command string, opts ConsentOptions) error {
	if opts.Yes {
		return nil
	}
	envToken := opts.ConsentEnvVar
	if envToken == "" {
		envToken = "SYMERASEME_CONSENT"
	}
	envFile := opts.ConsentFileEnvVar
	if envFile == "" {
		envFile = "SYMERASEME_CONSENT_FILE"
	}

	// 1. explicit --consent token
	if opts.ConsentToken != "" {
		if err := VerifyToken(command, opts.ConsentToken); err == nil {
			_ = ConsumeToken(opts.ConsentToken)
			return nil
		}
		return ErrConsentDenied
	}
	// 2. explicit --consent-file
	if opts.ConsentFile != "" {
		tok, err := ReadConsentFile(opts.ConsentFile)
		if err == nil && tok != "" {
			if verr := VerifyToken(command, tok); verr == nil {
				_ = ConsumeToken(tok)
				return nil
			}
		}
		return ErrConsentDenied
	}
	// 3. env file
	if v := os.Getenv(envFile); v != "" {
		if tok, err := ReadConsentFile(v); err == nil && tok != "" {
			if verr := VerifyToken(command, tok); verr == nil {
				_ = ConsumeToken(tok)
				return nil
			}
		}
		return ErrConsentDenied
	}
	// 4. env token
	if v := os.Getenv(envToken); v != "" {
		if err := VerifyToken(command, v); err == nil {
			_ = ConsumeToken(v)
			return nil
		}
		return ErrConsentDenied
	}
	// 5. interactive
	if opts.Interactive {
		if TTYPrompt(fmt.Sprintf("Destructive command '%s' requires consent. Proceed?", command)) {
			return nil
		}
	}
	return ErrConsentDenied
}
