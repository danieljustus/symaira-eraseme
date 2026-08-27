// Package identity holds the symeraseme identity, profile, consent and
// secrets layers in the Go port. It is the Go equivalent of the Python
// src/symeraseme/core/{identity,consent,secrets,config}.py modules and
// keeps the same on-disk formats so an existing Python-created profile
// and its secret entries are readable by the Go build.
//
// File layout on disk (matches the Python port byte-for-byte so a
// profile written by `symeraseme init-profile` is readable by the
// Go port and vice-versa):
//
//	identity.encrypted  :  JSON header line + "\n" + AES-256-GCM
//	                       ciphertext.  The header is the JSON object
//	                       {"version": 2, "nonce": "<hex>",
//	                        "algorithm": "AES-256-GCM"} which is also
//	                       used as the additional authenticated data
//	                       (AAD) when encrypting / decrypting.
//	consent_<hash>.json :  one file per issued consent token, stored
//	                       in the data directory.  The hash is the
//	                       first 16 hex chars of sha256(token).
//	secrets             :  the identity *master key* is held via the
//	                       OS keychain (best effort) with a passphrase
//	                       env-var fallback.  Never written to disk.
package identity

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
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
type Address struct {
	Street     string  `json:"street"`
	City       string  `json:"city"`
	PostalCode string  `json:"postal_code"`
	Country    string  `json:"country"`
	State      *string `json:"state,omitempty"`
	ValidFrom  *string `json:"valid_from,omitempty"` // ISO date
	ValidTo    *string `json:"valid_to,omitempty"`   // ISO date
}

// Profile is the in-memory identity profile model.  It mirrors the
// IdentityProfile Pydantic model in src/symeraseme/registry/schema.py.
// JSON tags match the Python pydantic dump (model_dump) so a profile
// written by the Python port round-trips through Go.
type Profile struct {
	FullName       string    `json:"full_name"`
	NameVariants   []string  `json:"name_variants,omitempty"`
	DateOfBirth    *string   `json:"date_of_birth,omitempty"` // ISO date
	Addresses      []Address `json:"addresses,omitempty"`
	EmailAddresses []string  `json:"email_addresses,omitempty"`
	PhoneNumbers   []string  `json:"phone_numbers,omitempty"`
	Jurisdictions  []string  `json:"jurisdictions,omitempty"`
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
// path, honouring SYMERASEME_IDENTITY_PATH, SYMERASEME_DATA_DIR and
// the same precedence as the Python config module.
func DefaultProfilePath() (string, error) {
	if v := os.Getenv("SYMERASEME_IDENTITY_PATH"); v != "" {
		return expand(v), nil
	}
	dataDir := defaultDataDir()
	if v, ok := os.LookupEnv("SYMERASEME_DATA_DIR"); ok {
		// Match Python: when the data directory is explicitly
		// overridden, the profile lives next to the event store
		// (not in the config dir).  The unset branch keeps the
		// config-dir default for backward compatibility.
		_ = v
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

// resolvePath returns the canonical path for a profile.  An empty
// path triggers DefaultProfilePath.
func resolvePath(path string) (string, error) {
	if path == "" {
		return DefaultProfilePath()
	}
	return expand(path), nil
}

// SaveProfile serialises and encrypts the profile to disk using the
// master key from MasterKeySource (creating one if none exists).
// Returns the absolute path of the file.
func SaveProfile(p *Profile, path string) (string, error) {
	target, err := resolvePath(path)
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(filepath.Dir(target), ProfileDirMode); err != nil {
		return "", fmt.Errorf("identity: mkdir: %w", err)
	}
	key, err := GetOrCreateMasterKey()
	if err != nil {
		return "", err
	}
	plaintext, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		return "", fmt.Errorf("identity: marshal profile: %w", err)
	}
	out, err := encryptProfile(plaintext, key)
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(target, out, ProfileFileMode); err != nil {
		return "", fmt.Errorf("identity: write profile: %w", err)
	}
	// Invalidate cache so the next LoadProfile re-reads the file.
	profileCacheMu.Lock()
	delete(profileCache, target)
	profileCacheMu.Unlock()
	return target, nil
}

// LoadProfile reads, decrypts and validates the profile at path (or
// the default location when path is empty).  The on-disk format is
// the Python format so a profile written by the Python port is
// readable here.  A missing file yields ErrProfileNotFound.
func LoadProfile(path string) (*Profile, error) {
	target, err := resolvePath(path)
	if err != nil {
		return nil, err
	}
	profileCacheMu.Lock()
	if cached, ok := profileCache[target]; ok {
		profileCacheMu.Unlock()
		return cached, nil
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
	profileCacheMu.Lock()
	profileCache[target] = &p
	profileCacheMu.Unlock()
	return &p, nil
}

// DeleteProfile removes the profile file from disk and clears the
// master key from the keychain (best effort).  Mirrors delete_profile.
func DeleteProfile(path string) error {
	target, err := resolvePath(path)
	if err != nil {
		return err
	}
	if err := os.Remove(target); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	profileCacheMu.Lock()
	delete(profileCache, target)
	profileCacheMu.Unlock()
	_ = DeleteMasterKey() // best effort
	return nil
}

// ProfileExists reports whether a profile is on disk at the given
// path (or the default location).
func ProfileExists(path string) bool {
	target, err := resolvePath(path)
	if err != nil {
		return false
	}
	_, err = os.Stat(target)
	return err == nil
}

// HashProfile returns a deterministic, non-reversible SHA-256 of the
// canonical JSON form of the profile.  Mirrors hash_profile() in
// identity.py and is used for audit-trail entries.
func HashProfile(p *Profile) string {
	canon, _ := json.Marshal(p) // json.Marshal sorts map keys; the
	// Pydantic model_dump() output keeps field order, but the hash
	// is over the canonical form so field order must not change the
	// digest.  Build a canonical map explicitly.
	canon = canonicalJSON(p)
	sum := sha256.Sum256(canon)
	return hex.EncodeToString(sum[:])
}

// canonicalJSON renders p as a JSON object with sorted keys.  This
// matches json.dumps(..., sort_keys=True) used in hash_profile().
func canonicalJSON(p *Profile) []byte {
	// json.MarshalIndent without indent + map keys sorted
	m := map[string]any{
		"full_name": p.FullName,
	}
	if len(p.NameVariants) > 0 {
		m["name_variants"] = p.NameVariants
	}
	if p.DateOfBirth != nil {
		m["date_of_birth"] = *p.DateOfBirth
	}
	if len(p.Addresses) > 0 {
		addrs := make([]map[string]any, 0, len(p.Addresses))
		for _, a := range p.Addresses {
			am := map[string]any{
				"street":      a.Street,
				"city":        a.City,
				"postal_code": a.PostalCode,
				"country":     a.Country,
			}
			if a.State != nil {
				am["state"] = *a.State
			}
			if a.ValidFrom != nil {
				am["valid_from"] = *a.ValidFrom
			}
			if a.ValidTo != nil {
				am["valid_to"] = *a.ValidTo
			}
			addrs = append(addrs, am)
		}
		m["addresses"] = addrs
	}
	if len(p.EmailAddresses) > 0 {
		m["email_addresses"] = p.EmailAddresses
	}
	if len(p.PhoneNumbers) > 0 {
		m["phone_numbers"] = p.PhoneNumbers
	}
	if len(p.Jurisdictions) > 0 {
		m["jurisdictions"] = p.Jurisdictions
	}
	out, _ := json.Marshal(m) //nolint:errcheck // marshalling a map[string]any of basic types cannot fail
	return out
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
	out := make([]byte, 0, len(headerJSON)+1+len(ciphertext))
	out = append(out, headerJSON...)
	out = append(out, '\n')
	out = append(out, ciphertext...)
	return out, nil
}

// decryptProfile reverses encryptProfile.  The AAD must match the
// header bytes, so any tampering with the header is caught.
func decryptProfile(raw []byte) ([]byte, *ProfileEnvelope, error) {
	// Find the first newline separating the header JSON from the
	// ciphertext.  json.Marshal never emits a raw newline so the
	// first 0x0A byte is the separator.
	nl := -1
	for i, b := range raw {
		if b == '\n' {
			nl = i
			break
		}
	}
	if nl < 0 {
		return nil, nil, fmt.Errorf("%w: no header separator", ErrProfileCorrupt)
	}
	headerJSON := raw[:nl]
	ciphertext := raw[nl+1:]
	var header ProfileEnvelope
	if err := json.Unmarshal(headerJSON, &header); err != nil {
		return nil, nil, fmt.Errorf("%w: header: %v", ErrProfileCorrupt, err)
	}
	if header.Version == legacyV0ProfileVersion {
		// v0 had no AAD; surface the dedicated error so callers
		// can show a tailored message.
		return nil, &header, ErrLegacyV0Unsupported
	}
	nonce, err := hex.DecodeString(header.Nonce)
	if err != nil {
		return nil, &header, fmt.Errorf("%w: nonce: %v", ErrProfileCorrupt, err)
	}
	key, err := GetOrCreateMasterKey()
	if err != nil {
		return nil, &header, err
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, &header, fmt.Errorf("identity: aes cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, &header, fmt.Errorf("identity: gcm: %w", err)
	}
	plain, err := gcm.Open(nil, nonce, ciphertext, headerJSON)
	if err != nil {
		return nil, &header, fmt.Errorf("%w: %v", ErrProfileCorrupt, err)
	}
	return plain, &header, nil
}

// nowUnixSeconds is a small seam for tests; default returns time.Now().
var nowUnixSeconds = func() int64 { return time.Now().Unix() }
