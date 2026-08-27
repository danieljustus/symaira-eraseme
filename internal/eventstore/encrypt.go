// Package eventstore (encrypt.go) implements the at-rest envelope
// described in docs/event-store.md §5. The format is whole-file
// Fernet (AES-256-GCM) with three header versions:
//
//	V1 (legacy)  : "SYMERASEME_ENCv1\n" + Fernet token
//	                key = PBKDF2-HMAC-SHA256(master, fixed-salt, 600k)
//	V2           : "SYMERASEME_ENCv2\n" + 16-byte random salt + Fernet token
//	                key = PBKDF2-HMAC-SHA256(master, salt, 600k)
//	V3 (current) : "SYMERASEME_ENCv3\n" + 16-byte random salt + Fernet token
//	                key = HKDF-SHA256(master, salt, info="symeraseme-db-encryption-v3")
//
// The master key is supplied by the caller (a 32-byte key fetched
// from the identity vault). Tests can inject a master key via
// MasterKeyProvider.Set / a package-level variable; production code
// wires the identity keyring through SetMasterKeyProvider.
package eventstore

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"golang.org/x/crypto/hkdf"
	"golang.org/x/crypto/pbkdf2"
)

// RandomBytes returns n cryptographically-secure random bytes.
// Mirrors Python's secrets.token_bytes.
func RandomBytes(n int) []byte {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		// crypto/rand only errors when the system RNG is broken —
		// we'd rather panic than silently emit predictable bytes.
		panic(fmt.Sprintf("eventstore: rand.Read failed: %v", err))
	}
	return b
}

// defaultNowSeconds returns the current Unix time in seconds.
func defaultNowSeconds() int64 { return time.Now().Unix() }

// Encryption envelope constants (mirror db_encryption.py).
var (
	EncHeaderV1 = []byte("SYMERASEME_ENCv1\n") // 15 bytes
	EncMagicV2  = []byte("SYMERASEME_ENCv2\n") // 16 bytes
	EncMagicV3  = []byte("SYMERASEME_ENCv3\n") // 16 bytes
)

// SaltLen is the length (bytes) of the per-file salt for V2/V3.
const SaltLen = 16

// PBKDF2Iterations mirrors db_encryption.PBKDF2_ITERATIONS.
const PBKDF2Iterations = 600_000

// PBKDF2FixedSalt is the V1 fixed salt. It is also used as the
// salt for V2 *file* derivation in some legacy paths; the V3
// format always draws a fresh random salt.
var PBKDF2FixedSalt = []byte("symeraseme-db-encryption-v1")

// HKDFInfoV3 is the V3 info parameter for HKDF.
var HKDFInfoV3 = []byte("symeraseme-db-encryption-v3")

// ErrUnrecognizedHeader is returned when the file header is not a
// known encryption version.
var ErrUnrecognizedHeader = errors.New("eventstore: unrecognized encryption header")

// ErrMasterKeyUnavailable is returned when no master key is
// configured (and the caller requested encryption / decryption).
var ErrMasterKeyUnavailable = errors.New("eventstore: identity master key unavailable")

// MasterKeyProvider returns the 32-byte master key on demand. The
// function may be called concurrently and is expected to be cheap
// (in production it consults the OS keyring).
type MasterKeyProvider func() ([]byte, error)

// masterKeyMu guards masterKeyFn.
var masterKeyMu sync.RWMutex
var masterKeyFn MasterKeyProvider

// SetMasterKeyProvider registers the function used to obtain the
// identity master key. Pass nil to clear.
func SetMasterKeyProvider(fn MasterKeyProvider) {
	masterKeyMu.Lock()
	masterKeyFn = fn
	masterKeyMu.Unlock()
}

// currentMasterKey returns the cached master key (must be 32 bytes).
func currentMasterKey() ([]byte, error) {
	masterKeyMu.RLock()
	fn := masterKeyFn
	masterKeyMu.RUnlock()
	if fn == nil {
		return nil, ErrMasterKeyUnavailable
	}
	mk, err := fn()
	if err != nil {
		return nil, err
	}
	if len(mk) != 32 {
		return nil, fmt.Errorf("eventstore: master key must be 32 bytes, got %d", len(mk))
	}
	return mk, nil
}

// DeriveKeyPBKDF2 returns a 32-byte key via PBKDF2-HMAC-SHA256 with
// 600,000 iterations.  Mirrors cryptography.hazmat KDF.pbkdf2 used
// in db_encryption.py.
func DeriveKeyPBKDF2(master, salt []byte) []byte {
	return pbkdf2.Key(master, salt, PBKDF2Iterations, 32, sha256.New)
}

// DeriveKeyHKDF returns a 32-byte key via HKDF-SHA256 with the
// given info and salt.  Mirrors HKDF(algorithm=SHA256, length=32,
// info=..., salt=...).
func DeriveKeyHKDF(master, salt, info []byte) ([]byte, error) {
	r := hkdf.New(sha256.New, master, salt, info)
	out := make([]byte, 32)
	if _, err := r.Read(out); err != nil {
		return nil, err
	}
	return out, nil
}

// fernetKeyFromBytes returns the URL-safe base64 encoding used as
// the Fernet "key" (the 32-byte AES key encoded as base64url is
// what Fernet expects when wrapping the token, because Fernet
// does its own key derivation in the cryptography library).
func fernetKeyFromBytes(b []byte) []byte {
	out := make([]byte, base64.RawURLEncoding.EncodedLen(len(b)))
	base64.RawURLEncoding.Encode(out, b)
	return out
}

// DetectVersion reads the header of a file and returns 1, 2, 3 or
// (0, false) when the file is not encrypted.
func DetectVersion(raw []byte) (int, bool) {
	switch {
	case bytes.HasPrefix(raw, EncHeaderV1):
		return 1, true
	case bytes.HasPrefix(raw, EncMagicV2):
		return 2, true
	case bytes.HasPrefix(raw, EncMagicV3):
		return 3, true
	}
	return 0, false
}

// IsEncrypted reports whether the file at path starts with a known
// encryption header.  Returns false (and no error) for missing or
// zero-length files.
func IsEncrypted(path string) (bool, error) {
	f, err := os.Open(path) //nolint:gosec // path supplied by caller
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return false, nil
		}
		return false, err
	}
	defer f.Close()
	head := make([]byte, max3(len(EncHeaderV1), len(EncMagicV2)+SaltLen, len(EncMagicV3)+SaltLen))
	n, err := f.Read(head)
	if err != nil && n == 0 {
		return false, nil
	}
	_, ok := DetectVersion(head[:n])
	return ok, nil
}

func max3(a, b, c int) int {
	m := a
	if b > m {
		m = b
	}
	if c > m {
		m = c
	}
	return m
}

// DecryptFile decrypts the encrypted file at srcPath and returns
// the plaintext bytes.  Supports V1, V2, V3 reads (auto-detected).
func DecryptFile(srcPath string) ([]byte, error) {
	raw, err := os.ReadFile(srcPath) //nolint:gosec // path supplied by caller
	if err != nil {
		return nil, err
	}
	return DecryptBytes(raw)
}

// DecryptBytes decrypts an encrypted byte slice.  Supports V1, V2,
// V3 (auto-detected).
func DecryptBytes(raw []byte) ([]byte, error) {
	version, ok := DetectVersion(raw)
	if !ok {
		return nil, ErrUnrecognizedHeader
	}
	var (
		salt    []byte
		keyFrom int
	)
	switch version {
	case 1:
		keyFrom = len(EncHeaderV1)
		salt = PBKDF2FixedSalt
	case 2:
		keyFrom = len(EncMagicV2) + SaltLen
		salt = raw[len(EncMagicV2) : len(EncMagicV2)+SaltLen]
	case 3:
		keyFrom = len(EncMagicV3) + SaltLen
		salt = raw[len(EncMagicV3) : len(EncMagicV3)+SaltLen]
	default:
		return nil, ErrUnrecognizedHeader
	}
	token := raw[keyFrom:]

	mk, err := currentMasterKey()
	if err != nil {
		return nil, err
	}
	var keyBytes []byte
	if version >= 3 {
		k, err := DeriveKeyHKDF(mk, salt, HKDFInfoV3)
		if err != nil {
			return nil, err
		}
		keyBytes = k
	} else {
		keyBytes = DeriveKeyPBKDF2(mk, salt)
	}
	return DecryptFernetToken(token, keyBytes)
}

// EncryptBytesV3 returns the V3-encrypted form of plaintext using
// the supplied master key.  Always uses a fresh random salt.
func EncryptBytesV3(plaintext, master []byte) ([]byte, error) {
	salt := RandomBytes(SaltLen)
	key, err := DeriveKeyHKDF(master, salt, HKDFInfoV3)
	if err != nil {
		return nil, err
	}
	token, err := EncryptFernetToken(plaintext, key)
	if err != nil {
		return nil, err
	}
	out := make([]byte, 0, len(EncMagicV3)+SaltLen+len(token))
	out = append(out, EncMagicV3...)
	out = append(out, salt...)
	out = append(out, token...)
	return out, nil
}

// EncryptBytesV2 returns the V2-encrypted form (PBKDF2, random salt).
func EncryptBytesV2(plaintext, master []byte) ([]byte, error) {
	salt := RandomBytes(SaltLen)
	key := DeriveKeyPBKDF2(master, salt)
	token, err := EncryptFernetToken(plaintext, key)
	if err != nil {
		return nil, err
	}
	out := make([]byte, 0, len(EncMagicV2)+SaltLen+len(token))
	out = append(out, EncMagicV2...)
	out = append(out, salt...)
	out = append(out, token...)
	return out, nil
}

// --------------------------------------------------------------------
// Fernet token: 0x80 | 8-byte big-endian timestamp | 16-byte nonce |
// ciphertext | 32-byte HMAC-SHA256 over all preceding bytes
// --------------------------------------------------------------------

// FernetToken is the unencrypted intermediate form of a Fernet
// token (used by EncryptFernetToken / DecryptFernetToken so callers
// can build/inspect tokens without going through the file envelope).
type FernetToken struct {
	Version   byte
	Timestamp uint64
	Nonce     []byte
	Plaintext []byte
}

// EncryptFernetToken builds a Fernet token (version, timestamp,
// nonce, ciphertext, HMAC) from plaintext using the 32-byte key.
func EncryptFernetToken(plaintext, key []byte) ([]byte, error) {
	if len(key) != 32 {
		return nil, fmt.Errorf("eventstore: fernet key must be 32 bytes, got %d", len(key))
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	nonce := RandomBytes(gcm.NonceSize())
	ct := gcm.Seal(nil, nonce, plaintext, nil)

	tok := &FernetToken{
		Version:   0x80,
		Timestamp: uint64(nowSeconds()),
		Nonce:     nonce,
		Plaintext: ct,
	}
	body := make([]byte, 0, 1+8+len(nonce)+len(ct)+32)
	body = append(body, tok.Version)
	var ts [8]byte
	binary.BigEndian.PutUint64(ts[:], tok.Timestamp)
	body = append(body, ts[:]...)
	body = append(body, tok.Nonce...)
	body = append(body, tok.Plaintext...)

	mac := hmac.New(sha256.New, key)
	mac.Write(body)
	body = append(body, mac.Sum(nil)...)
	return body, nil
}

// DecryptFernetToken reverses EncryptFernetToken.  Returns the
// plaintext (or an error on HMAC mismatch / bad version).
func DecryptFernetToken(token, key []byte) ([]byte, error) {
	if len(key) != 32 {
		return nil, fmt.Errorf("eventstore: fernet key must be 32 bytes, got %d", len(key))
	}
	if len(token) < 1+8+12+16+32 { // minimal Fernet token
		return nil, errors.New("eventstore: fernet token too short")
	}
	if token[0] != 0x80 {
		return nil, fmt.Errorf("eventstore: unsupported fernet version 0x%02x", token[0])
	}
	ts := binary.BigEndian.Uint64(token[1:9])
	nonce := token[9 : 9+12]
	ciphertext := token[9+12 : len(token)-32]
	gotMAC := token[len(token)-32:]

	mac := hmac.New(sha256.New, key)
	mac.Write(token[:len(token)-32])
	wantMAC := mac.Sum(nil)
	if !hmac.Equal(gotMAC, wantMAC) {
		return nil, errors.New("eventstore: fernet HMAC mismatch")
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	pt, err := gcm.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return nil, fmt.Errorf("eventstore: fernet decrypt: %w", err)
	}
	_ = ts // timestamp not used in plaintext extraction
	return pt, nil
}

// --------------------------------------------------------------------
// At-rest helpers: decrypt-to-temp / encrypt-from-temp
// --------------------------------------------------------------------

// EncryptedTemp is the bookkeeping entry for a decrypted temp file
// that needs to be re-encrypted on close.  The key is the canonical
// path of the original encrypted DB; the value is the path of the
// decrypted temp file.
var encryptedTemps sync.Map // map[string]string (origPath → tmpPath)

// RegisterTemp remembers an orig → tmp path so WriteEncrypted can
// re-encrypt on close.
func RegisterTemp(origPath, tmpPath string) {
	encryptedTemps.Store(origPath, tmpPath)
}

// ForgetTemp clears the bookkeeping entry.
func ForgetTemp(origPath string) {
	encryptedTemps.Delete(origPath)
}

// DecryptToTemp decrypts srcPath to a freshly-created temp file
// (mode 0600) and registers the pair.  Returns the temp file path.
func DecryptToTemp(srcPath, tmpDir string) (string, error) {
	raw, err := os.ReadFile(srcPath) //nolint:gosec // caller path
	if err != nil {
		return "", err
	}
	plain, err := DecryptBytes(raw)
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(tmpDir, 0o700); err != nil {
		return "", err
	}
	f, err := os.CreateTemp(tmpDir, "symeraseme_decrypted_*.db")
	if err != nil {
		return "", err
	}
	tmpPath := f.Name()
	if _, err := f.Write(plain); err != nil {
		_ = f.Close()
		_ = os.Remove(tmpPath)
		return "", err
	}
	if err := f.Close(); err != nil {
		_ = os.Remove(tmpPath)
		return "", err
	}
	if err := os.Chmod(tmpPath, 0o600); err != nil {
		return "", err
	}
	RegisterTemp(srcPath, tmpPath)
	return tmpPath, nil
}

// WriteEncrypted re-encrypts the temp file (if changed) back to the
// original path.  Always writes a V3 envelope with a fresh random
// salt.  Removes the temp file.
func WriteEncrypted(origPath string) error {
	v, ok := encryptedTemps.Load(origPath)
	if !ok {
		return nil
	}
	tmpPath := v.(string)
	plain, err := os.ReadFile(tmpPath) //nolint:gosec // our own temp file
	if err != nil {
		_ = os.Remove(tmpPath)
		encryptedTemps.Delete(origPath)
		return err
	}
	mk, err := currentMasterKey()
	if err != nil {
		_ = os.Remove(tmpPath)
		encryptedTemps.Delete(origPath)
		return err
	}
	out, err := EncryptBytesV3(plain, mk)
	if err != nil {
		_ = os.Remove(tmpPath)
		encryptedTemps.Delete(origPath)
		return err
	}
	if err := os.WriteFile(origPath, out, 0o600); err != nil { //nolint:gosec // our own file
		_ = os.Remove(tmpPath)
		encryptedTemps.Delete(origPath)
		return err
	}
	_ = os.Remove(tmpPath)
	encryptedTemps.Delete(origPath)
	return nil
}

// MigrateV1ToV3 re-encrypts a V1 file to V3 in place.
func MigrateV1ToV3(path string) error {
	raw, err := os.ReadFile(path) //nolint:gosec // caller path
	if err != nil {
		return err
	}
	plain, err := DecryptBytes(raw)
	if err != nil {
		return err
	}
	mk, err := currentMasterKey()
	if err != nil {
		return err
	}
	out, err := EncryptBytesV3(plain, mk)
	if err != nil {
		return err
	}
	return os.WriteFile(path, out, 0o600) //nolint:gosec // caller path
}

// MigrateV2ToV3 re-encrypts a V2 file to V3 in place.
func MigrateV2ToV3(path string) error {
	raw, err := os.ReadFile(path) //nolint:gosec // caller path
	if err != nil {
		return err
	}
	plain, err := DecryptBytes(raw)
	if err != nil {
		return err
	}
	mk, err := currentMasterKey()
	if err != nil {
		return err
	}
	out, err := EncryptBytesV3(plain, mk)
	if err != nil {
		return err
	}
	return os.WriteFile(path, out, 0o600) //nolint:gosec // caller path
}

// EncryptExisting encrypts an existing plaintext file in place
// using V3 (fresh random salt).  Returns the new file size.
func EncryptExisting(path string) error {
	plain, err := os.ReadFile(path) //nolint:gosec // caller path
	if err != nil {
		return err
	}
	mk, err := currentMasterKey()
	if err != nil {
		return err
	}
	out, err := EncryptBytesV3(plain, mk)
	if err != nil {
		return err
	}
	return os.WriteFile(path, out, 0o600) //nolint:gosec // caller path
}

// nowSeconds returns the current Unix time in seconds.  Overridable
// from tests to make Fernet tokens deterministic.
var nowSeconds = defaultNowSeconds

// --------------------------------------------------------------------
// OpenEncrypted: full at-rest plumbing
// --------------------------------------------------------------------

// OpenEncrypted opens an encrypted DB at encPath.  If the file is
// not yet encrypted (plain SQLite), it remains a plain file — but
// the returned Store registers the path so Close will re-encrypt
// it.  The decrypted plaintext lives in tmpDir (mode 0600).
func OpenEncrypted(encPath, tmpDir string) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(encPath), 0o700); err != nil {
		return nil, err
	}
	raw, err := os.ReadFile(encPath) //nolint:gosec // caller path
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	if errors.Is(err, os.ErrNotExist) || len(raw) == 0 {
		// Brand new file: create a plain SQLite and register for
		// encrypt-on-close.
		s, err := Open(encPath)
		if err != nil {
			return nil, err
		}
		RegisterTemp(encPath, encPath)
		return s, nil
	}
	version, ok := DetectVersion(raw)
	if !ok {
		// Plaintext DB: open in place, register for encrypt-on-close.
		s, err := Open(encPath)
		if err != nil {
			return nil, err
		}
		RegisterTemp(encPath, encPath)
		return s, nil
	}
	// Encrypted — transparently migrate to V3, then decrypt to temp.
	switch version {
	case 1:
		if err := MigrateV1ToV3(encPath); err != nil {
			return nil, err
		}
	case 2:
		if err := MigrateV2ToV3(encPath); err != nil {
			return nil, err
		}
	}
	tmpPath, err := DecryptToTemp(encPath, tmpDir)
	if err != nil {
		return nil, err
	}
	s, err := Open(tmpPath)
	if err != nil {
		_ = os.Remove(tmpPath)
		ForgetTemp(encPath)
		return nil, err
	}
	// Replace the path so callers see the canonical encrypted path.
	s.path = encPath
	return s, nil
}

// CloseAt closes the Store and re-encrypts the temp file back to
// the original path (if a temp was registered).  The V3 envelope is
// always used on the way out.
func (s *Store) CloseAt(encPath string) error {
	_ = s.db.Close()
	if v, ok := encryptedTemps.Load(encPath); ok {
		tmpPath := v.(string)
		if tmpPath == encPath {
			// Plaintext-on-disk path: encrypt in place.
			if err := EncryptExisting(encPath); err != nil {
				return err
			}
			encryptedTemps.Delete(encPath)
			return nil
		}
		// Re-encrypt tmp → encPath.
		plain, err := os.ReadFile(tmpPath) //nolint:gosec // our own temp file
		if err != nil {
			_ = os.Remove(tmpPath)
			encryptedTemps.Delete(encPath)
			return err
		}
		mk, err := currentMasterKey()
		if err != nil {
			_ = os.Remove(tmpPath)
			encryptedTemps.Delete(encPath)
			return err
		}
		out, err := EncryptBytesV3(plain, mk)
		if err != nil {
			_ = os.Remove(tmpPath)
			encryptedTemps.Delete(encPath)
			return err
		}
		if err := os.WriteFile(encPath, out, 0o600); err != nil { //nolint:gosec // caller path
			_ = os.Remove(tmpPath)
			encryptedTemps.Delete(encPath)
			return err
		}
		_ = os.Remove(tmpPath)
		// Also remove WAL/-shm siblings next to the temp file (the
		// plain SQLite we just closed may have created them).
		_ = os.Remove(tmpPath + "-wal")
		_ = os.Remove(tmpPath + "-shm")
		encryptedTemps.Delete(encPath)
	}
	return nil
}
