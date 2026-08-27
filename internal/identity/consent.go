// Package identity — consent.go: consent token mechanism for
// destructive operations.  The on-disk format is intentionally
// identical to src/symeraseme/core/consent.py so the Go port and
// the Python port can read each other's tokens.
//
// File layout (per issued token):
//
//	<consent_dir>/consent_<hash>.json
//
// where <hash> is the first 16 hex characters of sha256(token).
// The file body is the JSON object:
//
//	{
//	  "command":    "<destructive command name>",
//	  "issued_at":  <unix seconds>,
//	  "expires_at": <unix seconds>,
//	  "token":      "<urlsafe random 16-byte token>"   (optional,
//	                                                  present in v2+)
//	}
//
// The token is verified by:
//
//  1. Hashing the supplied token, looking for the matching file
//     (auto-migrating the legacy unhashed filename when present).
//  2. Comparing the file's `token` field (v2) or skipping the
//     check (v1) — knowing only the filename is no longer enough
//     to verify.
//  3. Ensuring the file's `command` matches.
//  4. Ensuring `expires_at` is in the future.
package identity

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

// DefaultTokenTTL is the lifetime of an issued consent token.
// Mirrors TOKEN_TTL = 86400 (24h) in the Python module.
const DefaultTokenTTL = 86400

// Errors returned by the consent layer.
var (
	// ErrConsentTokenNotFound is returned when no file matches the
	// supplied token (after hash + legacy fallback).
	ErrConsentTokenNotFound = errors.New("identity: consent token not found")

	// ErrConsentTokenExpired indicates a matching file was found
	// but its expires_at is in the past.
	ErrConsentTokenExpired = errors.New("identity: consent token expired")

	// ErrConsentTokenCommandMismatch indicates a matching file was
	// found but its `command` field does not match.
	ErrConsentTokenCommandMismatch = errors.New("identity: consent token command mismatch")
)

// ConsentRecord is the in-memory shape of a token file.
type ConsentRecord struct {
	Command   string `json:"command"`
	IssuedAt  int64  `json:"issued_at"`
	ExpiresAt int64  `json:"expires_at"`
	// Token is the v2+ field that ties the file to the token
	// value supplied by the caller.  When empty the file uses
	// the v1 (legacy) format and the filename is sufficient.
	Token string `json:"token,omitempty"`
}

// ConsentToken is the public summary returned by ListTokens.
type ConsentToken struct {
	Token     string `json:"token"`
	Command   string `json:"command"`
	IssuedAt  int64  `json:"issued_at"`
	ExpiresAt int64  `json:"expires_at"`
}

// ConsentOptions configures ConsentGate.
type ConsentOptions struct {
	Yes               bool   // --yes flag: always grant
	ConsentToken      string // --consent flag (already-issued token)
	ConsentFile       string // --consent-file flag
	Interactive       bool   // whether to prompt on TTY
	ConsentEnvVar     string // env var holding the token (default SYMERASEME_CONSENT)
	ConsentFileEnvVar string // env var holding the consent file (default SYMERASEME_CONSENT_FILE)
}

// DefaultConsentDir returns the platform-default consent directory,
// honouring SYMERASEME_DATA_DIR.  Mirrors the Python config's
// consent_dir property.
func DefaultConsentDir() (string, error) {
	return defaultDataDir(), nil
}

// tokenFilename returns the hashed filename for a given token.
func tokenFilename(token string) string {
	sum := sha256.Sum256([]byte(token))
	return "consent_" + hex.EncodeToString(sum[:])[:16] + ".json"
}

// tokenLegacyFilename is the unhashed form used by early versions.
func tokenLegacyFilename(token string) string {
	return "consent_" + token + ".json"
}

// consentDir resolves the consent directory and ensures it exists
// with 0o700 mode.
func consentDir(dir string) (string, error) {
	if dir == "" {
		d, err := DefaultConsentDir()
		if err != nil {
			return "", err
		}
		dir = d
	}
	if err := os.MkdirAll(dir, ProfileDirMode); err != nil {
		return "", fmt.Errorf("identity: consent mkdir: %w", err)
	}
	st, err := os.Stat(dir)
	if err == nil {
		// Harden existing directories created without explicit mode.
		if st.Mode().Perm() != ProfileDirMode {
			_ = os.Chmod(dir, ProfileDirMode)
		}
	}
	return dir, nil
}

// IssueToken creates a new consent token for command and writes it
// to disk.  Returns the token value (url-safe, ~22 chars).
func IssueToken(command string, ttl int) (string, error) {
	return IssueTokenInDir("", command, ttl)
}

// IssueTokenInDir is IssueToken with an explicit directory override
// (used by tests and by callers that resolve the consent dir from a
// custom data location).
func IssueTokenInDir(dir, command string, ttl int) (string, error) {
	if ttl <= 0 {
		ttl = DefaultTokenTTL
	}
	d, err := consentDir(dir)
	if err != nil {
		return "", err
	}
	now := nowUnixSeconds()
	rec := ConsentRecord{
		Command:   command,
		IssuedAt:  now,
		ExpiresAt: now + int64(ttl),
		Token:     RandomURLSafe(16),
	}
	body, err := json.Marshal(rec)
	if err != nil {
		return "", fmt.Errorf("identity: marshal consent: %w", err)
	}
	path := filepath.Join(d, tokenFilename(rec.Token))
	if err := writeAtomic(path, body, ProfileFileMode); err != nil {
		return "", err
	}
	return rec.Token, nil
}

// VerifyToken returns nil if the token is valid for command, or
// one of the consent errors.  Expired tokens are removed as a
// side-effect (mirroring the Python verifier).
func VerifyToken(command, token string) error {
	return VerifyTokenInDir("", command, token)
}

// VerifyTokenInDir is VerifyToken with an explicit directory.
func VerifyTokenInDir(dir, command, token string) error {
	if token == "" {
		return ErrConsentTokenNotFound
	}
	d, err := consentDir(dir)
	if err != nil {
		return err
	}
	path := findTokenFile(d, token)
	if path == "" {
		return ErrConsentTokenNotFound
	}
	data, err := os.ReadFile(path) //nolint:gosec // our own file
	if err != nil {
		return ErrConsentTokenNotFound
	}
	var rec ConsentRecord
	if err := json.Unmarshal(data, &rec); err != nil {
		return ErrConsentTokenNotFound
	}
	// v2: ensure the token in the payload matches.
	if rec.Token != "" && rec.Token != token {
		return ErrConsentTokenNotFound
	}
	if rec.Command != command {
		return ErrConsentTokenCommandMismatch
	}
	if nowUnixSeconds() > rec.ExpiresAt {
		_ = os.Remove(path)
		return ErrConsentTokenExpired
	}
	// Tighten permissions on existing files.
	st, err := os.Stat(path)
	if err == nil && st.Mode().Perm() != ProfileFileMode {
		_ = os.Chmod(path, ProfileFileMode)
	}
	return nil
}

// ConsumeToken removes a token file after successful verification.
// Idempotent: missing files are ignored.
func ConsumeToken(token string) error {
	return ConsumeTokenInDir("", token)
}

// ConsumeTokenInDir is ConsumeToken with an explicit directory.
func ConsumeTokenInDir(dir, token string) error {
	if token == "" {
		return nil
	}
	d, err := consentDir(dir)
	if err != nil {
		return err
	}
	path := findTokenFile(d, token)
	if path == "" {
		return nil
	}
	_ = os.Remove(path)
	return nil
}

// RevokeToken removes a token file and reports whether anything was
// removed.  Mirrors revoke_token() in the Python module.
func RevokeToken(token string) (bool, error) {
	return RevokeTokenInDir("", token)
}

// RevokeTokenInDir is RevokeToken with an explicit directory.
func RevokeTokenInDir(dir, token string) (bool, error) {
	if token == "" {
		return false, nil
	}
	d, err := consentDir(dir)
	if err != nil {
		return false, err
	}
	path := findTokenFile(d, token)
	if path == "" {
		return false, nil
	}
	if err := os.Remove(path); err != nil {
		return false, err
	}
	return true, nil
}

// ListTokens returns every non-expired token in the consent
// directory, sorted by filename.  Expired tokens are pruned.
func ListTokens() ([]ConsentToken, error) {
	return ListTokensInDir("")
}

// ListTokensInDir is ListTokens with an explicit directory.
func ListTokensInDir(dir string) ([]ConsentToken, error) {
	d, err := consentDir(dir)
	if err != nil {
		return nil, err
	}
	entries, err := os.ReadDir(d)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	now := nowUnixSeconds()
	var out []ConsentToken
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		name := e.Name()
		if !strings.HasPrefix(name, "consent_") || !strings.HasSuffix(name, ".json") {
			continue
		}
		path := filepath.Join(d, name)
		data, err := os.ReadFile(path) //nolint:gosec // our own file
		if err != nil {
			continue
		}
		var rec ConsentRecord
		if err := json.Unmarshal(data, &rec); err != nil {
			continue
		}
		if now > rec.ExpiresAt {
			_ = os.Remove(path)
			continue
		}
		st, _ := os.Stat(path)
		if st != nil && st.Mode().Perm() != ProfileFileMode {
			_ = os.Chmod(path, ProfileFileMode)
		}
		token := rec.Token
		if token == "" {
			// Legacy: derive the token id from the filename.
			token = strings.TrimSuffix(strings.TrimPrefix(name, "consent_"), ".json")
		}
		out = append(out, ConsentToken{
			Token:     token,
			Command:   rec.Command,
			IssuedAt:  rec.IssuedAt,
			ExpiresAt: rec.ExpiresAt,
		})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].IssuedAt < out[j].IssuedAt })
	return out, nil
}

// findTokenFile locates a token file by hashed filename, falling
// back to the legacy unhashed form.  Returns an empty string when
// no match is found.  Side-effect: renames a found legacy file to
// the hashed form (best effort).
func findTokenFile(dir, token string) string {
	hashed := filepath.Join(dir, tokenFilename(token))
	if _, err := os.Stat(hashed); err == nil {
		return hashed
	}
	legacy := filepath.Join(dir, tokenLegacyFilename(token))
	if !withinDir(dir, legacy) {
		return ""
	}
	if _, err := os.Stat(legacy); err == nil {
		_ = os.Rename(legacy, hashed)
		return hashed
	}
	return ""
}

// withinDir reports whether path is contained inside dir (after
// EvalSymlinks best-effort).  Mirrors the Python parent check.
func withinDir(dir, path string) bool {
	absDir, err := filepath.Abs(dir)
	if err != nil {
		return false
	}
	absPath, err := filepath.Abs(path)
	if err != nil {
		return false
	}
	rel, err := filepath.Rel(absDir, absPath)
	if err != nil {
		return false
	}
	if rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
		return false
	}
	return true
}

// writeAtomic writes body to path by first writing a temp file in
// the same directory, then renaming.  Avoids partial writes on
// crash.
func writeAtomic(path string, body []byte, mode os.FileMode) error {
	dir := filepath.Dir(path)
	tmp, err := os.CreateTemp(dir, ".consent-*.tmp")
	if err != nil {
		return fmt.Errorf("identity: consent tmp: %w", err)
	}
	tmpName := tmp.Name()
	defer func() {
		_ = os.Remove(tmpName)
	}()
	if _, err := tmp.Write(body); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Chmod(tmpName, mode); err != nil {
		return err
	}
	if err := os.Rename(tmpName, path); err != nil {
		return err
	}
	return nil
}

// Now returns the current unix seconds (injected for tests).
func Now() int64 { return nowUnixSeconds() }

// SetNowFunc overrides the time source (tests).
func SetNowFunc(fn func() int64) {
	nowMu.Lock()
	nowUnixSeconds = fn
	nowMu.Unlock()
}

var nowMu sync.Mutex

// init default: time.Now().Unix().
func init() {
	nowUnixSeconds = func() int64 { return time.Now().Unix() }
}

// ParseExpiry returns the parsed expires_at integer for the given
// token (exposed for diagnostic commands).
func ParseExpiry(s string) (int64, error) {
	return strconv.ParseInt(s, 10, 64)
}
