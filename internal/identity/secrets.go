// Package identity — secrets.go: master-key resolution and secret
// URI resolution.  Replaces the Python keyring call in identity.py
// and the symvault:// / vault:// URI handling in secrets.py.
//
// Master-key lookup order (highest priority first):
//
//  1. In-process cache (set by a previous successful lookup, e.g.
//     when Bootstrap wires the key into the event store).
//  2. Environment variable — SYMERASEME_IDENTITY_MASTER_KEY (hex).
//     This is the preferred way to feed a passphrase-style secret
//     into headless / CI deployments without a keyring.
//  3. Environment variable — SYMVAULT_PASSPHRASE.  The corekit/
//     symvault ecosystem convention for a vault-derived master
//     passphrase (the secret was fetched from the vault and exposed
//     as this env var by the calling operator). The value is
//     derived with scrypt to produce a deterministic AES-256 key.
//  4. OS keychain via the `keyring` library (best effort).  When
//     no keychain backend is available, fall through.
//
// If none of the above produces a key, GetExistingMasterKey
// returns ErrMasterKeyMissing.  GetOrCreateMasterKey generates a
// fresh AES-256 key, writes it to the keychain (best effort) and
// returns it.
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
	"sync"

	"github.com/danieljustus/symaira-corekit/envutil"
	"github.com/zalando/go-keyring"
	"golang.org/x/crypto/scrypt"
)

// Master-key env vars.  Honoured by GetExistingMasterKey in the
// listed order.  The hex form is the canonical on-wire form so
// tools that copy the key out of the keychain (or vault) can set
// it directly.
const (
	// EnvMasterKeyHex is the direct hex-encoded 32-byte master key.
	EnvMasterKeyHex = "SYMERASEME_IDENTITY_MASTER_KEY"

	// EnvSymvaultPassphrase is the corekit/symvault ecosystem
	// convention: the operator fetches the master passphrase from
	// the vault and exposes it as this env var.  Hashed via
	// SHA-256 to produce a 32-byte AES-256 key.
	EnvSymvaultPassphrase = "SYMVAULT_PASSPHRASE"
)

// identityPassphraseSalt domain-separates passphrase-derived identity keys
// from all other uses of scrypt. It is public format metadata, not a secret.
var identityPassphraseSalt = []byte("symeraseme-identity-master-key-v1")

// keyCache caches the resolved master key in-process.  The cache
// survives until the process exits, mirroring the Python
// _get_existing_master_key fast path.
var (
	keyCacheMu sync.Mutex
	keyCache   []byte
)

// SetMasterKey explicitly stores a master key in the in-process
// cache.  Tests use this to inject a deterministic key without
// touching the keychain or environment.  A zero-length key is
// treated as "clear" and removes the cache entry.
func SetMasterKey(key []byte) {
	keyCacheMu.Lock()
	defer keyCacheMu.Unlock()
	if len(key) == 0 {
		keyCache = nil
		return
	}
	if len(key) != KeyLength {
		// Hash anything else down to 32 bytes deterministically.
		sum := sha256.Sum256(key)
		keyCache = make([]byte, KeyLength)
		copy(keyCache, sum[:])
		return
	}
	keyCache = append([]byte(nil), key...)
}

// GenerateMasterKey returns a fresh 32-byte AES-256 key.
func GenerateMasterKey() ([]byte, error) {
	k := make([]byte, KeyLength)
	if _, err := rand.Read(k); err != nil {
		return nil, fmt.Errorf("identity: generate master key: %w", err)
	}
	return k, nil
}

// GetExistingMasterKey returns the stored master key, or
// ErrMasterKeyMissing when no source can supply one.  Use this in
// the read / decrypt path so a missing key fails fast instead of
// silently minting a new one (which would render the encrypted
// profile unreadable).
func GetExistingMasterKey() ([]byte, error) {
	keyCacheMu.Lock()
	cached := keyCache
	keyCacheMu.Unlock()
	if len(cached) == KeyLength {
		return append([]byte(nil), cached...), nil
	}

	// 1. Direct hex form (canonical wire form).
	if hexKey := envutil.Getenv(EnvMasterKeyHex); hexKey != "" {
		raw, err := hex.DecodeString(hexKey)
		if err != nil {
			return nil, fmt.Errorf("identity: %s: invalid hex: %w", EnvMasterKeyHex, err)
		}
		if len(raw) != KeyLength {
			return nil, fmt.Errorf("identity: %s must be %d bytes, got %d", EnvMasterKeyHex, KeyLength, len(raw))
		}
		SetMasterKey(raw)
		return raw, nil
	}

	// 2. Symvault-passphrase form (memory-hard derivation).
	if pass := envutil.Getenv(EnvSymvaultPassphrase); pass != "" {
		key, err := scrypt.Key([]byte(pass), identityPassphraseSalt, 1<<15, 8, 1, KeyLength)
		if err != nil {
			return nil, fmt.Errorf("identity: derive master key: %w", err)
		}
		SetMasterKey(key)
		return key, nil
	}

	// 3. OS keychain.  Best effort; any error falls through to
	//    ErrMasterKeyMissing so the caller can show a clean
	//    "run init-profile" message.
	if k, err := keyring.Get(ServiceName, KeyringUsername); err == nil && k != "" {
		raw, derr := hex.DecodeString(k)
		if derr == nil && len(raw) == KeyLength {
			SetMasterKey(raw)
			return raw, nil
		}
	}

	return nil, ErrMasterKeyMissing
}

// GetOrCreateMasterKey returns the stored master key, or generates
// and stores a new one when no key is present.  Use this in the
// write / encrypt path (save_profile, first-time setup).
func GetOrCreateMasterKey() ([]byte, error) {
	if k, err := GetExistingMasterKey(); err == nil {
		return k, nil
	}
	k, err := GenerateMasterKey()
	if err != nil {
		return nil, err
	}
	// Best-effort write to the keychain.  An unwritable keychain
	// is not fatal — the operator may be relying on an env var
	// for the next process start.
	_ = setKeyringBestEffort(hex.EncodeToString(k))
	SetMasterKey(k)
	return k, nil
}

// DeleteMasterKey clears the keychain entry (best effort) and the
// in-process cache.  Mirrors _delete_master_key.
func DeleteMasterKey() error {
	keyCacheMu.Lock()
	keyCache = nil
	keyCacheMu.Unlock()
	if err := keyring.Delete(ServiceName, KeyringUsername); err != nil {
		// keyring returns ErrNotFound when the entry is absent;
		// treat that as success so the function is idempotent.
		if errors.Is(err, keyring.ErrNotFound) {
			return nil
		}
		return err
	}
	return nil
}

func setKeyringBestEffort(hexKey string) error {
	// Test stub: a misconfigured keychain on Linux/Windows CI is
	// common; fall through silently.
	defer func() { _ = recover() }() //nolint:revive
	return keyring.Set(ServiceName, KeyringUsername, hexKey)
}

// EncryptProfileWithKey is a helper used by tests and the bootstrap
// flow to re-encrypt a profile with a caller-supplied key (e.g. a
// fresh key generated by GetOrCreateMasterKey when no on-disk
// profile exists yet).
func EncryptProfileWithKey(plaintext, key []byte) ([]byte, error) {
	return encryptProfile(plaintext, key)
}

// DecryptProfileWithKey is a helper used by tests to verify a
// ciphertext produced by a different key yields the expected
// ErrProfileCorrupt path.
func DecryptProfileWithKey(raw, key []byte) ([]byte, error) {
	plain, _, err := decryptProfileWithKey(raw, key)
	return plain, err
}

// decryptProfileWithKey is the key-injecting variant of
// decryptProfile; it skips the master-key lookup.
func decryptProfileWithKey(raw, key []byte) ([]byte, *ProfileEnvelope, error) {
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
		return nil, &header, ErrLegacyV0Unsupported
	}
	nonce, err := hex.DecodeString(header.Nonce)
	if err != nil {
		return nil, &header, fmt.Errorf("%w: nonce: %v", ErrProfileCorrupt, err)
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
