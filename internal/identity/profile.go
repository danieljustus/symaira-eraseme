// Package identity holds the symeraseme identity, profile, consent and
// secrets layers in the Go port. It is the Go equivalent of the Python
// src/symeraseme/core/{identity,consent,secrets,config}.py modules and
// keeps the same on-disk formats so an existing Python-created profile
// and its secret entries are readable by the Go build.
//
// File layout on disk:
//
//	identity.encrypted  :  JSON header line + "\n" + AES-256-GCM
//	                       ciphertext.  The header is the JSON object
//	                       {"version": 2, "nonce": "<hex>",
//	                        "algorithm": "AES-256-GCM"} which is also
//	                       used as the additional authenticated data
//	                       (AAD) when encrypting / decrypting.
//	                       This is the canonical Go filename for writes.
//	identity.enc        :  Archived Python-era filename. Path discovery
//	                       transparently reads identity.enc when
//	                       identity.encrypted is absent, without deleting
//	                       the source file or stranding existing Go users.
//	consent_<hash>.json :  one file per issued consent token, stored
//	                       in the data directory.  The hash is the
//	                       first 16 hex chars of sha256(token).
//	secrets             :  the identity *master key* is held via the
//	                       OS keychain with environment variable fallbacks.
//	                       Never written to disk.
package identity

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Profile constants — mirror the Python identity.py module.
const (
	// ServiceName is the keychain service identifier for the
	// identity master key.  Mirrors SERVICE_NAME = "symeraseme".
	ServiceName = "symeraseme"

	// KeyringUsername is the keychain account for the master key.
	// Mirrors KEYRING_USERNAME = "identity-master-key".
	KeyringUsername = "identity-master-key"

	// KeyLength is the master key size in bytes (AES-256).
	KeyLength = 32

	// NonceLength is the AES-GCM nonce size in bytes.
	NonceLength = 12

	// ProfileFileMode is the on-disk permission for the profile.
	ProfileFileMode os.FileMode = 0o600

	// ProfileDirMode is the on-disk permission for the profile dir.
	ProfileDirMode os.FileMode = 0o700

	// currentProfileVersion is the on-disk envelope version.
	// Bump only when the wire format changes; legacy versions
	// remain readable for migration by the eventstore port.
	currentProfileVersion = 2

	legacyV0ProfileVersion = 0
)

// Errors returned by the profile layer.
var (
	// ErrProfileNotFound is returned by LoadProfile when the
	// profile file is missing.  Mirrors Python's FileNotFoundError.
	ErrProfileNotFound = errors.New("identity: profile not found")

	// ErrMasterKeyMissing is returned when the master key cannot be
	// located through any of the configured resolution layers.
	// Mirrors Python's RuntimeError from _get_existing_master_key.
	ErrMasterKeyMissing = errors.New("identity: master key missing")

	// ErrProfileCorrupt is returned when decryption succeeds but
	// the inner JSON does not validate.  Mirrors the chained
	// RuntimeError from load_profile.
	ErrProfileCorrupt = errors.New("identity: profile corrupt")

	// ErrLegacyV0Unsupported mirrors the Python message for
	// legacy v0 profiles (no AAD) — they are intentionally not
	// readable for security reasons.
	ErrLegacyV0Unsupported = errors.New("identity: legacy v0 profile (no AAD) is no longer supported")
)

// Address mirrors the Pydantic Address model in registry/schema.py.
// All fields are always serialized (no omitempty) to ensure exact byte
// and semantic compatibility with Python Pydantic output.
type Address struct {
	Street     string  `json:"street"`
	City       string  `json:"city"`
	PostalCode string  `json:"postal_code"`
	Country    string  `json:"country"`
	State      *string `json:"state"`
	ValidFrom  *string `json:"valid_from"` // ISO date
	ValidTo    *string `json:"valid_to"`   // ISO date
}

// Profile is the in-memory identity profile model.  It mirrors the
// IdentityProfile Pydantic model in src/symeraseme/registry/schema.py.
// JSON tags match the Python Pydantic model_dump_json output: all fields
// are present, empty lists are serialized as [], and unset optional fields
// are serialized as null (no omitempty omission).
type Profile struct {
	FullName       string    `json:"full_name"`
	NameVariants   []string  `json:"name_variants"`
	DateOfBirth    *string   `json:"date_of_birth"` // ISO date
	Addresses      []Address `json:"addresses"`
	EmailAddresses []string  `json:"email_addresses"`
	PhoneNumbers   []string  `json:"phone_numbers"`
	Jurisdictions  []string  `json:"jurisdictions"`
}

func (p *Profile) normalize() {
	if p.NameVariants == nil {
		p.NameVariants = []string{}
	}
	if p.Addresses == nil {
		p.Addresses = []Address{}
	}
	if p.EmailAddresses == nil {
		p.EmailAddresses = []string{}
	}
	if p.PhoneNumbers == nil {
		p.PhoneNumbers = []string{}
	}
	if p.Jurisdictions == nil {
		p.Jurisdictions = []string{}
	}
}

func (p *Profile) clone() *Profile {
	if p == nil {
		return nil
	}
	cp := *p
	if p.NameVariants != nil {
		cp.NameVariants = append([]string(nil), p.NameVariants...)
	}
	if p.DateOfBirth != nil {
		dob := *p.DateOfBirth
		cp.DateOfBirth = &dob
	}
	if p.Addresses != nil {
		cp.Addresses = make([]Address, len(p.Addresses))
		for i, a := range p.Addresses {
			addr := a
			if a.State != nil {
				st := *a.State
				addr.State = &st
			}
			if a.ValidFrom != nil {
				vf := *a.ValidFrom
				addr.ValidFrom = &vf
			}
			if a.ValidTo != nil {
				vt := *a.ValidTo
				addr.ValidTo = &vt
			}
			cp.Addresses[i] = addr
		}
	}
	if p.EmailAddresses != nil {
		cp.EmailAddresses = append([]string(nil), p.EmailAddresses...)
	}
	if p.PhoneNumbers != nil {
		cp.PhoneNumbers = append([]string(nil), p.PhoneNumbers...)
	}
	if p.Jurisdictions != nil {
		cp.Jurisdictions = append([]string(nil), p.Jurisdictions...)
	}
	return &cp
}

// ProfileEnvelope is the JSON header line written ahead of the
// ciphertext.  Mirrors the header dict produced by identity.py.
type ProfileEnvelope struct {
	Version   int    `json:"version"`
	Nonce     string `json:"nonce"`
	Algorithm string `json:"algorithm"`
}

// profileCache holds the in-process profile cache, indexed by the
// canonical file path so multiple Stores (e.g. tests) don't fight
// over a single global.
var (
	profileCacheMu sync.Mutex
	profileCache   = map[string]*Profile{}
)

// DefaultProfilePath returns the platform-default identity profile
// path for writes, honouring SYMERASEME_IDENTITY_PATH, SYMERASEME_DATA_DIR and
// the same precedence as the Python config module. The default write path is
// always the canonical Go filename "identity.encrypted".
func DefaultProfilePath() (string, error) {
	if v := os.Getenv("SYMERASEME_IDENTITY_PATH"); v != "" {
		return expand(v), nil
	}
	dataDir := defaultDataDir()
	if v := os.Getenv("SYMERASEME_DATA_DIR"); v != "" {
		return filepath.Join(dataDir, "identity.encrypted"), nil
	}
	configDir := defaultConfigDir()
	return filepath.Join(configDir, "identity.encrypted"), nil
}

// defaultDataDir mirrors the Python Config.resolved_data_dir.
func defaultDataDir() string {
	if v := os.Getenv("SYMERASEME_DATA_DIR"); v != "" {
		return expand(v)
	}
	return expand("~/.local/share/symeraseme")
}

// defaultConfigDir mirrors the Python Config.resolved_config_dir.
func defaultConfigDir() string {
	if v := os.Getenv("SYMERASEME_CONFIG_DIR"); v != "" {
		return expand(v)
	}
	return expand("~/.config/symeraseme")
}

func expand(p string) string {
	if p == "" {
		return p
	}
	if p[0] == '~' {
		home, err := os.UserHomeDir()
		if err == nil {
			if len(p) == 1 {
				return home
			}
			if p[1] == '/' {
				return filepath.Join(home, p[2:])
			}
		}
	}
	return p
}

// resolveReadPath resolves the profile file to read.
//  1. If an explicit path is provided and exists, return it.
//     If it doesn't exist, check the alternate extension (.encrypted <-> .enc).
//  2. If SYMERASEME_IDENTITY_PATH is set and exists, return it (checking alternate extension).
//  3. Otherwise, check the default directory (data dir if overridden, else config dir):
//     - Return identity.encrypted if it exists (avoids stranding existing Go users).
//     - Return identity.enc if it exists (transparent Python discovery without deleting source).
//     - If neither exists, return the canonical Go path identity.encrypted.
func resolveReadPath(path string) (string, error) {
	if path != "" {
		target := expand(path)
		if _, err := os.Stat(target); err == nil {
			return target, nil
		}
		base := filepath.Base(target)
		dir := filepath.Dir(target)
		switch base {
		case "identity.encrypted":
			alt := filepath.Join(dir, "identity.enc")
			if _, err := os.Stat(alt); err == nil {
				return alt, nil
			}
		case "identity.enc":
			alt := filepath.Join(dir, "identity.encrypted")
			if _, err := os.Stat(alt); err == nil {
				return alt, nil
			}
		}
		return target, nil
	}

	if v := os.Getenv("SYMERASEME_IDENTITY_PATH"); v != "" {
		target := expand(v)
		if _, err := os.Stat(target); err == nil {
			return target, nil
		}
		base := filepath.Base(target)
		dir := filepath.Dir(target)
		switch base {
		case "identity.encrypted":
			alt := filepath.Join(dir, "identity.enc")
			if _, err := os.Stat(alt); err == nil {
				return alt, nil
			}
		case "identity.enc":
			alt := filepath.Join(dir, "identity.encrypted")
			if _, err := os.Stat(alt); err == nil {
				return alt, nil
			}
		}
		return target, nil
	}

	var dir string
	if v := os.Getenv("SYMERASEME_DATA_DIR"); v != "" {
		dir = defaultDataDir()
	} else {
		dir = defaultConfigDir()
	}

	goPath := filepath.Join(dir, "identity.encrypted")
	if _, err := os.Stat(goPath); err == nil {
		return goPath, nil
	}

	pyPath := filepath.Join(dir, "identity.enc")
	if _, err := os.Stat(pyPath); err == nil {
		return pyPath, nil
	}

	return goPath, nil
}

// resolveWritePath returns the path to write to.
// An empty path triggers DefaultProfilePath() ("identity.encrypted").
func resolveWritePath(path string) (string, error) {
	if path == "" {
		return DefaultProfilePath()
	}
	return expand(path), nil
}

// SaveProfile serialises and encrypts the profile to disk using the
// master key from GetExistingMasterKey. Callers must have initialized
// the master key prior to saving. Writes to the canonical Go path
// "identity.encrypted" when path is empty, without deleting any
// existing Python-era "identity.enc" file.
// Profile writes use same-directory tempfile, chmod 0600, file fsync,
// atomic rename, and directory fsync to prevent file truncation or corruption.
func SaveProfile(p *Profile, path string) (string, error) {
	target, err := resolveWritePath(path)
	if err != nil {
		return "", err
	}
	target = filepath.Clean(target)
	dir := filepath.Dir(target)
	if err := os.MkdirAll(dir, ProfileDirMode); err != nil {
		return "", fmt.Errorf("identity: mkdir: %w", err)
	}

	// SaveProfile strictly requires an existing master key.
	// Decrypt/read and save paths do not mint keys; key creation is explicit initialization only.
	key, err := GetExistingMasterKey()
	if err != nil {
		return "", err
	}

	p.normalize()
	plaintext, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		return "", fmt.Errorf("identity: marshal profile: %w", err)
	}
	out, err := encryptProfile(plaintext, key)
	if err != nil {
		return "", err
	}

	tmpFile, err := os.CreateTemp(dir, "."+filepath.Base(target)+".tmp-*")
	if err != nil {
		return "", fmt.Errorf("identity: create temp profile: %w", err)
	}
	tmpPath := tmpFile.Name()
	keepTmp := false
	defer func() {
		if !keepTmp {
			_ = tmpFile.Close()
			_ = os.Remove(tmpPath)
		}
	}()

	if err := tmpFile.Chmod(ProfileFileMode); err != nil {
		return "", fmt.Errorf("identity: chmod profile temp: %w", err)
	}

	if _, err := tmpFile.Write(out); err != nil {
		return "", fmt.Errorf("identity: write profile temp: %w", err)
	}

	if err := tmpFile.Sync(); err != nil {
		return "", fmt.Errorf("identity: fsync profile temp: %w", err)
	}

	if err := tmpFile.Close(); err != nil {
		return "", fmt.Errorf("identity: close profile temp: %w", err)
	}

	if err := os.Rename(tmpPath, target); err != nil {
		return "", fmt.Errorf("identity: atomic rename profile: %w", err)
	}
	keepTmp = true

	if d, err := os.Open(dir); err == nil {
		_ = d.Sync()
		_ = d.Close()
	}

	// Invalidate cache so the next LoadProfile re-reads the file.
	profileCacheMu.Lock()
	delete(profileCache, target)
	base := filepath.Base(target)
	switch base {
	case "identity.encrypted":
		delete(profileCache, filepath.Join(dir, "identity.enc"))
	case "identity.enc":
		delete(profileCache, filepath.Join(dir, "identity.encrypted"))
	}
	profileCacheMu.Unlock()
	return target, nil
}

// InitProfile explicitly initializes the master key (if not already present)
// and saves the identity profile. This is the explicit initialization path.
// If profile saving fails and the key was newly created by this invocation,
// the newly created key is rolled back from keychain and cache.
func InitProfile(p *Profile, path string) (string, error) {
	_, existingErr := GetExistingMasterKey()
	keyExisted := existingErr == nil

	if _, err := InitMasterKey(); err != nil {
		return "", err
	}

	target, err := SaveProfile(p, path)
	if err != nil {
		if !keyExisted {
			_ = DeleteMasterKey()
		}
		return "", err
	}
	return target, nil
}

// LoadProfile reads, decrypts and validates the profile at path (or
// discovered default location when path is empty). The on-disk format is
// Python-compatible so profiles written by both Python and Go are readable here.
// Missing profiles yield ErrProfileNotFound. Decrypt paths strictly perform
// read-only key lookup and never mint or replace keys.
func LoadProfile(path string) (*Profile, error) {
	target, err := resolveReadPath(path)
	if err != nil {
		return nil, err
	}
	target = filepath.Clean(target)
	profileCacheMu.Lock()
	if cached, ok := profileCache[target]; ok {
		profileCacheMu.Unlock()
		return cached.clone(), nil
	}
	profileCacheMu.Unlock()

	if _, err := os.Stat(target); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, ErrProfileNotFound
		}
		return nil, fmt.Errorf("identity: stat profile: %w", err)
	}
	raw, err := os.ReadFile(target) //nolint:gosec // path supplied by caller
	if err != nil {
		return nil, fmt.Errorf("identity: read profile: %w", err)
	}
	plain, header, err := decryptProfile(raw)
	if err != nil {
		return nil, err
	}
	if header.Version == legacyV0ProfileVersion {
		return nil, ErrLegacyV0Unsupported
	}
	var p Profile
	if err := json.Unmarshal(plain, &p); err != nil {
		return nil, fmt.Errorf("%w: %v", ErrProfileCorrupt, err)
	}
	p.normalize()
	profileCacheMu.Lock()
	profileCache[target] = p.clone()
	profileCacheMu.Unlock()
	return p.clone(), nil
}

// DeleteProfile removes the profile file from disk and clears the
// master key from the keychain (best effort). Mirrors delete_profile.
// When called with default discovery (path == ""), it deletes both
// "identity.encrypted" and legacy "identity.enc" to prevent resurrection.
// When called with an explicit custom path, deletion stays strictly scoped.
func DeleteProfile(path string) error {
	var targets []string
	if path != "" {
		targets = []string{filepath.Clean(expand(path))}
	} else if v := os.Getenv("SYMERASEME_IDENTITY_PATH"); v != "" {
		target := filepath.Clean(expand(v))
		targets = append(targets, target)
		base := filepath.Base(target)
		dir := filepath.Dir(target)
		switch base {
		case "identity.encrypted":
			targets = append(targets, filepath.Join(dir, "identity.enc"))
		case "identity.enc":
			targets = append(targets, filepath.Join(dir, "identity.encrypted"))
		}
	} else {
		var dir string
		if v := os.Getenv("SYMERASEME_DATA_DIR"); v != "" {
			dir = defaultDataDir()
		} else {
			dir = defaultConfigDir()
		}
		targets = append(targets, filepath.Join(dir, "identity.encrypted"), filepath.Join(dir, "identity.enc"))
	}

	var firstErr error
	for _, target := range targets {
		if err := os.Remove(target); err != nil && !errors.Is(err, os.ErrNotExist) {
			if firstErr == nil {
				firstErr = fmt.Errorf("identity: delete profile %s: %w", target, err)
			}
		}
		profileCacheMu.Lock()
		delete(profileCache, target)
		profileCacheMu.Unlock()
	}

	_ = DeleteMasterKey() // best effort
	return firstErr
}

// ProfileExists reports whether a profile is on disk at the given
// path (or discovered default location).
func ProfileExists(path string) bool {
	target, err := resolveReadPath(path)
	if err != nil {
		return false
	}
	_, err = os.Stat(target)
	return err == nil
}

// writePythonEscapedString writes a JSON string using Python's ensure_ascii=True escaping rules.
func writePythonEscapedString(sb *strings.Builder, s string) {
	sb.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			sb.WriteString(`\"`)
		case '\\':
			sb.WriteString(`\\`)
		case '\b':
			sb.WriteString(`\b`)
		case '\f':
			sb.WriteString(`\f`)
		case '\n':
			sb.WriteString(`\n`)
		case '\r':
			sb.WriteString(`\r`)
		case '\t':
			sb.WriteString(`\t`)
		default:
			if r < 0x20 || r == 0x7f {
				fmt.Fprintf(sb, `\u%04x`, r)
			} else if r < 0x80 {
				sb.WriteByte(byte(r))
			} else if r <= 0xffff {
				fmt.Fprintf(sb, `\u%04x`, r)
			} else {
				r -= 0x10000
				hi := 0xd800 + (r >> 10)
				lo := 0xdc00 + (r & 0x3ff)
				fmt.Fprintf(sb, `\u%04x\u%04x`, hi, lo)
			}
		}
	}
	sb.WriteByte('"')
}

// writePythonCanonical recursively serializes any value matching Python's
// json.dumps(val, sort_keys=True, ensure_ascii=True, separators=(", ", ": ")).
func writePythonCanonical(sb *strings.Builder, val any) {
	switch v := val.(type) {
	case nil:
		sb.WriteString("null")
	case bool:
		if v {
			sb.WriteString("true")
		} else {
			sb.WriteString("false")
		}
	case string:
		writePythonEscapedString(sb, v)
	case float64:
		if v == math.Trunc(v) && !math.IsNaN(v) && !math.IsInf(v, 0) {
			sb.WriteString(strconv.FormatInt(int64(v), 10))
		} else {
			sb.WriteString(strconv.FormatFloat(v, 'g', -1, 64))
		}
	case []any:
		sb.WriteByte('[')
		for i, elem := range v {
			if i > 0 {
				sb.WriteString(", ")
			}
			writePythonCanonical(sb, elem)
		}
		sb.WriteByte(']')
	case map[string]any:
		sb.WriteByte('{')
		keys := make([]string, 0, len(v))
		for k := range v {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for i, k := range keys {
			if i > 0 {
				sb.WriteString(", ")
			}
			writePythonEscapedString(sb, k)
			sb.WriteString(": ")
			writePythonCanonical(sb, v[k])
		}
		sb.WriteByte('}')
	}
}

// CanonicalGenericJSON serializes arbitrary data into canonical JSON matching
// Python's json.dumps(..., sort_keys=True, ensure_ascii=True).
func CanonicalGenericJSON(v any) ([]byte, error) {
	raw, err := json.Marshal(v)
	if err != nil {
		return nil, err
	}
	var generic any
	if err := json.Unmarshal(raw, &generic); err != nil {
		return nil, err
	}
	var sb strings.Builder
	writePythonCanonical(&sb, generic)
	return []byte(sb.String()), nil
}

// CanonicalJSON renders p as a canonical JSON object with sorted keys
// matching Python's json.dumps(profile.model_dump(mode="json"), sort_keys=True).
// Driven directly by the marshaled Profile so future fields cannot be silently omitted.
func CanonicalJSON(p *Profile) []byte {
	if p == nil {
		return []byte("null")
	}
	p.normalize()
	out, err := CanonicalGenericJSON(p)
	if err != nil {
		return nil
	}
	return out
}

// HashProfile returns a deterministic, non-reversible SHA-256 of the
// canonical JSON form of the profile. Mirrors hash_profile() in
// identity.py and is used for audit-trail entries.
func HashProfile(p *Profile) string {
	canon := CanonicalJSON(p)
	sum := sha256.Sum256(canon)
	return hex.EncodeToString(sum[:])
}

// ---------------------------------------------------------------------------
// On-disk encryption
// ---------------------------------------------------------------------------

// encryptProfile returns headerJSON + "\n" + AES-256-GCM(plaintext).
// The header bytes are the AAD, matching the Python implementation.
func encryptProfile(plaintext, key []byte) ([]byte, error) {
	if len(key) != KeyLength {
		return nil, fmt.Errorf("identity: master key must be %d bytes, got %d", KeyLength, len(key))
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("identity: aes cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("identity: gcm: %w", err)
	}
	nonce := make([]byte, NonceLength)
	if _, err := rand.Read(nonce); err != nil {
		return nil, fmt.Errorf("identity: nonce: %w", err)
	}
	header := ProfileEnvelope{
		Version:   currentProfileVersion,
		Nonce:     hex.EncodeToString(nonce),
		Algorithm: "AES-256-GCM",
	}
	headerJSON, err := json.Marshal(header)
	if err != nil {
		return nil, fmt.Errorf("identity: marshal envelope: %w", err)
	}
	ciphertext := gcm.Seal(nil, nonce, plaintext, headerJSON)
	if len(ciphertext) > math.MaxInt-1 || len(headerJSON) > math.MaxInt-1-len(ciphertext) {
		return nil, errors.New("identity: encrypted profile is too large")
	}
	out := make([]byte, 0, len(headerJSON)+1+len(ciphertext))
	out = append(out, headerJSON...)
	out = append(out, '\n')
	out = append(out, ciphertext...)
	return out, nil
}

// decryptProfile reverses encryptProfile. The AAD must match the
// header bytes, so any tampering with the header is caught.
// Decryption strictly calls GetExistingMasterKey; it NEVER mints a key.
func decryptProfile(raw []byte) ([]byte, *ProfileEnvelope, error) {
	separator := bytes.IndexByte(raw, '\n')
	if separator < 0 {
		return nil, nil, fmt.Errorf("%w: no header separator", ErrProfileCorrupt)
	}
	var header ProfileEnvelope
	if err := json.Unmarshal(raw[:separator], &header); err != nil {
		return nil, nil, fmt.Errorf("%w: header: %v", ErrProfileCorrupt, err)
	}
	if header.Version == legacyV0ProfileVersion {
		return nil, &header, ErrLegacyV0Unsupported
	}
	key, err := GetExistingMasterKey()
	if err != nil {
		return nil, nil, err
	}
	return decryptProfileWithKey(raw, key)
}

// nowUnixSeconds is a small seam for tests; default returns time.Now().
var nowUnixSeconds = func() int64 { return time.Now().Unix() }
