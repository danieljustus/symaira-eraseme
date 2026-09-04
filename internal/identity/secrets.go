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
//  4. OS keychain via the KeyringBackend interface (best effort for read,
//     mandatory durable persistence on explicit initialization).
//
// If none of the above produces a key, GetExistingMasterKey
// returns ErrMasterKeyMissing. Key creation is explicit initialization only
// via InitMasterKey; decrypt/read paths never mint or replace keys.
package identity

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
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

// KeyringBackend defines the interface for interacting with the OS keychain / keyring.
type KeyringBackend interface {
	Get(service, username string) (string, error)
	Set(service, username, password string) error
	Delete(service, username string) error
}

type defaultKeyringBackend struct{}

func (defaultKeyringBackend) Get(service, username string) (string, error) {
	return keyring.Get(service, username)
}

func (defaultKeyringBackend) Set(service, username, password string) error {
	return keyring.Set(service, username, password)
}

func (defaultKeyringBackend) Delete(service, username string) error {
	return keyring.Delete(service, username)
}

var (
	keyringMu      sync.RWMutex
	currentKeyring KeyringBackend = defaultKeyringBackend{}
)

// SetKeyringBackend allows tests or alternate runners to inject a fake/mock keyring backend.
// Passing nil resets to the default OS keyring.
func SetKeyringBackend(b KeyringBackend) {
	keyringMu.Lock()
	defer keyringMu.Unlock()
	if b == nil {
		currentKeyring = defaultKeyringBackend{}
	} else {
		currentKeyring = b
	}
}

func getKeyring() KeyringBackend {
	keyringMu.RLock()
	defer keyringMu.RUnlock()
	return currentKeyring
}

// keyCache caches the resolved master key in-process.  The cache
// survives until the process exits, mirroring the Python
// _get_existing_master_key fast path.
var (
	keyCacheMu sync.Mutex
	keyCache   []byte
)

// SetMasterKey explicitly stores a master key in the in-process cache.
// A zero-length key clears the cache entry. Malformed key lengths
// (anything other than 0 or KeyLength) are strictly rejected with an error.
func SetMasterKey(key []byte) error {
	keyCacheMu.Lock()
	defer keyCacheMu.Unlock()
	if len(key) == 0 {
		keyCache = nil
		return nil
	}
	if len(key) != KeyLength {
		return fmt.Errorf("identity: master key must be %d bytes, got %d", KeyLength, len(key))
	}
	keyCache = append([]byte(nil), key...)
	return nil
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
// all read / decrypt paths so a missing key fails fast instead of
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
		_ = SetMasterKey(raw)
		return raw, nil
	}

	// 2. Symvault-passphrase form (memory-hard derivation).
	if pass := envutil.Getenv(EnvSymvaultPassphrase); pass != "" {
		key, err := scrypt.Key([]byte(pass), identityPassphraseSalt, 1<<15, 8, 1, KeyLength)
		if err != nil {
			return nil, fmt.Errorf("identity: derive master key: %w", err)
		}
		_ = SetMasterKey(key)
		return key, nil
	}

	// 3. OS keychain via KeyringBackend.
	if k, err := getKeyring().Get(ServiceName, KeyringUsername); err == nil && k != "" {
		raw, derr := hex.DecodeString(k)
		if derr != nil {
			return nil, fmt.Errorf("identity: stored keychain master key is invalid hex: %w", derr)
		}
		if len(raw) != KeyLength {
			return nil, fmt.Errorf("identity: stored keychain master key must be %d bytes, got %d", KeyLength, len(raw))
		}
		_ = SetMasterKey(raw)
		return raw, nil
	}

	return nil, ErrMasterKeyMissing
}

// InitMasterKey explicitly creates and durably persists a fresh 32-byte
// master key. Key creation is explicit initialization only; callers must
// not rely on read/decrypt paths to mint keys. If durable keychain
// persistence fails, InitMasterKey fails closed and does not cache the key.
func InitMasterKey() ([]byte, error) {
	// If a valid key already exists (in memory, env, or keychain), return it.
	if k, err := GetExistingMasterKey(); err == nil {
		return k, nil
	}

	// Generate fresh AES-256 key.
	k, err := GenerateMasterKey()
	if err != nil {
		return nil, err
	}

	// Durable persistence is mandatory.
	hexKey := hex.EncodeToString(k)
	if err := getKeyring().Set(ServiceName, KeyringUsername, hexKey); err != nil {
		return nil, fmt.Errorf("identity: durable keychain persistence failed: %w", err)
	}

	if err := SetMasterKey(k); err != nil {
		return nil, err
	}
	return k, nil
}

// GetOrCreateMasterKey returns the stored master key, or explicitly
// initializes one when none is present.
func GetOrCreateMasterKey() ([]byte, error) {
	return InitMasterKey()
}

// DeleteMasterKey clears the keychain entry (best effort) and the
// in-process cache.  Mirrors _delete_master_key.
func DeleteMasterKey() error {
	keyCacheMu.Lock()
	keyCache = nil
	keyCacheMu.Unlock()
	if err := getKeyring().Delete(ServiceName, KeyringUsername); err != nil {
		// keyring returns ErrNotFound when the entry is absent;
		// treat that as success so the function is idempotent.
		if errors.Is(err, keyring.ErrNotFound) {
			return nil
		}
		return err
	}
	return nil
}

// EncryptProfileWithKey is a helper used by tests and the bootstrap
// flow to re-encrypt a profile with a caller-supplied key.
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
	if len(key) != KeyLength {
		return nil, nil, fmt.Errorf("identity: master key must be %d bytes, got %d", KeyLength, len(key))
	}
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
