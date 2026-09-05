// Package eventstore (encrypt.go) implements the at-rest envelope
// described in docs/event-store.md §5. The format is whole-file
// standard Fernet (AES-128-CBC + PKCS7 + HMAC-SHA256 over the Fernet frame,
// base64 URL-safe encoded) with three header versions:
//
//	V1 (legacy)  : "SYMERASEME_ENCv1\n" + Fernet token
//	                key = PBKDF2-HMAC-SHA256(master, fixed-salt, 600k)
//	V2           : "SYMERASEME_ENCv2\n" + 16-byte random salt + Fernet token
//	                key = PBKDF2-HMAC-SHA256(master, salt, 600k)
//	V3 (current) : "SYMERASEME_ENCv3\n" + 16-byte random salt + Fernet token
//	                key = HKDF-SHA256(master, salt, info="symeraseme-db-encryption-v3")
//
// Safe backward read compatibility is retained for accidental Go
// AES-256-GCM/HMAC payloads shipped prior to issue #798. When opened,
// accidental payloads are safely decrypted and migrated to the standard
// Fernet V3 envelope without data loss.
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
	"strings"
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
	EncHeaderV1 = []byte("SYMERASEME_ENCv1\n") // 17 bytes
	EncMagicV2  = []byte("SYMERASEME_ENCv2\n") // 17 bytes
	EncMagicV3  = []byte("SYMERASEME_ENCv3\n") // 17 bytes
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
// 600,000 iterations. Mirrors cryptography.hazmat KDF.pbkdf2 used
// in db_encryption.py.
func DeriveKeyPBKDF2(master, salt []byte) []byte {
	return pbkdf2.Key(master, salt, PBKDF2Iterations, 32, sha256.New)
}

// DeriveKeyHKDF returns a 32-byte key via HKDF-SHA256 with the
// given info and salt. Mirrors HKDF(algorithm=SHA256, length=32,
// info=..., salt=...).
func DeriveKeyHKDF(master, salt, info []byte) ([]byte, error) {
	r := hkdf.New(sha256.New, master, salt, info)
	out := make([]byte, 32)
	if _, err := r.Read(out); err != nil {
		return nil, err
	}
	return out, nil
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
// encryption header. Returns false (and no error) for missing or
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
// the plaintext bytes. Supports V1, V2, V3 reads (auto-detected).
func DecryptFile(srcPath string) ([]byte, error) {
	raw, err := os.ReadFile(srcPath) //nolint:gosec // path supplied by caller
	if err != nil {
		return nil, err
	}
	return DecryptBytes(raw)
}

// DecryptBytes decrypts an encrypted byte slice. Supports V1, V2,
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
		if len(raw) < keyFrom {
			return nil, errors.New("eventstore: encrypted V2 envelope is truncated")
		}
		salt = raw[len(EncMagicV2) : len(EncMagicV2)+SaltLen]
	case 3:
		keyFrom = len(EncMagicV3) + SaltLen
		if len(raw) < keyFrom {
			return nil, errors.New("eventstore: encrypted V3 envelope is truncated")
		}
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
// the supplied master key. Always uses standard Fernet and a fresh random salt.
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

// EncryptBytesV2 returns the V2-encrypted form (PBKDF2, random salt, standard Fernet).
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
// Fernet token: Standard Fernet (RFC / cryptography.fernet)
// Version (0x80) | 8-byte uint64 timestamp | 16-byte IV |
// AES-128-CBC ciphertext (PKCS7 padded) | 32-byte HMAC-SHA256
// Encoded as URL-safe base64 with padding.
// --------------------------------------------------------------------

// FernetToken represents the components of a Fernet token.
type FernetToken struct {
	Version    byte
	Timestamp  uint64
	Nonce      []byte // 16-byte IV for standard Fernet; 12-byte nonce for legacy Go
	Plaintext  []byte
	Ciphertext []byte
}

// EncryptFernetToken builds a standard Fernet token (RFC/cryptography.fernet:
// AES-128-CBC + PKCS7 + HMAC-SHA256, URL-safe base64 encoded) from plaintext
// using the 32-byte key.
//
// Key layout: key[:16] = signing key (HMAC-SHA256), key[16:] = encryption key (AES-128-CBC).
func EncryptFernetToken(plaintext, key []byte) ([]byte, error) {
	if len(key) != 32 {
		return nil, fmt.Errorf("eventstore: fernet key must be 32 bytes, got %d", len(key))
	}
	signingKey := key[:16]
	encryptionKey := key[16:]

	iv := RandomBytes(16)

	// PKCS7 padding to 16-byte block
	padLen := 16 - (len(plaintext) % 16)
	padded := make([]byte, len(plaintext)+padLen)
	copy(padded, plaintext)
	for i := len(plaintext); i < len(padded); i++ {
		padded[i] = byte(padLen)
	}

	block, err := aes.NewCipher(encryptionKey)
	if err != nil {
		return nil, err
	}
	mode := cipher.NewCBCEncrypter(block, iv)
	ciphertext := make([]byte, len(padded))
	mode.CryptBlocks(ciphertext, padded)

	// basic parts = 0x80 || timestamp (8 bytes BE) || IV (16 bytes) || ciphertext
	basic := make([]byte, 1+8+16+len(ciphertext))
	basic[0] = 0x80
	binary.BigEndian.PutUint64(basic[1:9], uint64(nowSeconds()))
	copy(basic[9:25], iv)
	copy(basic[25:], ciphertext)

	mac := hmac.New(sha256.New, signingKey)
	mac.Write(basic)
	sig := mac.Sum(nil)

	fullToken := append(basic, sig...)
	encoded := base64.URLEncoding.EncodeToString(fullToken)
	return []byte(encoded), nil
}

// DecryptFernetToken reverses EncryptFernetToken. It decrypts standard Fernet
// tokens (base64 URL-safe encoded) and retains safe read compatibility for
// accidentally shipped Go AES-GCM/HMAC tokens.
func DecryptFernetToken(token, key []byte) ([]byte, error) {
	if len(key) != 32 {
		return nil, fmt.Errorf("eventstore: fernet key must be 32 bytes, got %d", len(key))
	}

	// 1. Try standard Fernet decryption
	if pt, err := decryptStandardFernet(token, key); err == nil {
		return pt, nil
	}

	// 2. Fallback to accidental legacy Go AES-256-GCM token
	if pt, err := DecryptLegacyGoToken(token, key); err == nil {
		return pt, nil
	}

	// 3. If standard Fernet attempted but failed, return a meaningful error
	return nil, errors.New("eventstore: fernet token invalid, tampered, or unsupported format")
}

func decryptStandardFernet(token, key []byte) ([]byte, error) {
	signingKey := key[:16]
	encryptionKey := key[16:]

	var rawToken []byte
	clean := strings.TrimSpace(string(token))
	var decErr error
	rawToken, decErr = base64.URLEncoding.DecodeString(clean)
	if decErr != nil {
		rawToken, decErr = base64.RawURLEncoding.DecodeString(clean)
		if decErr != nil {
			rawToken, decErr = base64.StdEncoding.DecodeString(clean)
		}
	}
	if decErr != nil {
		// If base64 decode failed, check if token was passed as raw binary standard frame
		if len(token) >= 73 && token[0] == 0x80 {
			rawToken = token
		} else {
			return nil, decErr
		}
	}

	if len(rawToken) < 73 {
		return nil, errors.New("eventstore: fernet token too short")
	}
	if rawToken[0] != 0x80 {
		return nil, fmt.Errorf("eventstore: unsupported fernet version 0x%02x", rawToken[0])
	}

	// Verify HMAC-SHA256 with signingKey over frame excluding trailing 32-byte MAC
	mac := hmac.New(sha256.New, signingKey)
	mac.Write(rawToken[:len(rawToken)-32])
	wantMAC := mac.Sum(nil)
	gotMAC := rawToken[len(rawToken)-32:]
	if !hmac.Equal(gotMAC, wantMAC) {
		return nil, errors.New("eventstore: fernet HMAC mismatch")
	}

	iv := rawToken[9:25]
	ciphertext := rawToken[25 : len(rawToken)-32]
	if len(ciphertext) == 0 || len(ciphertext)%16 != 0 {
		return nil, errors.New("eventstore: fernet invalid ciphertext block length")
	}

	block, err := aes.NewCipher(encryptionKey)
	if err != nil {
		return nil, err
	}
	mode := cipher.NewCBCDecrypter(block, iv)
	padded := make([]byte, len(ciphertext))
	mode.CryptBlocks(padded, ciphertext)

	// PKCS7 unpad
	if len(padded) == 0 {
		return nil, errors.New("eventstore: empty decrypted block")
	}
	padLen := int(padded[len(padded)-1])
	if padLen < 1 || padLen > 16 || padLen > len(padded) {
		return nil, errors.New("eventstore: fernet invalid PKCS7 padding")
	}
	for i := len(padded) - padLen; i < len(padded); i++ {
		if padded[i] != byte(padLen) {
			return nil, errors.New("eventstore: fernet invalid PKCS7 padding bytes")
		}
	}
	return padded[:len(padded)-padLen], nil
}

// EncryptLegacyGoToken creates the accidental Go AES-256-GCM + outer HMAC token layout.
func EncryptLegacyGoToken(plaintext, key []byte) ([]byte, error) {
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

	body := make([]byte, 0, 1+8+len(nonce)+len(ct)+32)
	body = append(body, 0x80)
	var ts [8]byte
	binary.BigEndian.PutUint64(ts[:], uint64(nowSeconds()))
	body = append(body, ts[:]...)
	body = append(body, nonce...)
	body = append(body, ct...)

	mac := hmac.New(sha256.New, key)
	mac.Write(body)
	body = append(body, mac.Sum(nil)...)
	return body, nil
}

// DecryptLegacyGoToken decrypts the accidental Go AES-256-GCM + outer HMAC token layout.
func DecryptLegacyGoToken(token, key []byte) ([]byte, error) {
	if len(key) != 32 {
		return nil, fmt.Errorf("eventstore: fernet key must be 32 bytes, got %d", len(key))
	}
	if len(token) < 1+8+12+16+32 { // minimal accidental Go token = 69 bytes
		return nil, errors.New("eventstore: legacy token too short")
	}
	if token[0] != 0x80 {
		return nil, fmt.Errorf("eventstore: unsupported legacy version 0x%02x", token[0])
	}
	nonce := token[9 : 9+12]
	ciphertext := token[9+12 : len(token)-32]
	gotMAC := token[len(token)-32:]

	mac := hmac.New(sha256.New, key)
	mac.Write(token[:len(token)-32])
	wantMAC := mac.Sum(nil)
	if !hmac.Equal(gotMAC, wantMAC) {
		return nil, errors.New("eventstore: legacy HMAC mismatch")
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
		return nil, fmt.Errorf("eventstore: legacy decrypt: %w", err)
	}
	return pt, nil
}

// EncryptLegacyGoBytesV3 produces a V3 envelope containing an accidental Go AES-256-GCM token (for tests).
func EncryptLegacyGoBytesV3(plaintext, master []byte) ([]byte, error) {
	salt := RandomBytes(SaltLen)
	key, err := DeriveKeyHKDF(master, salt, HKDFInfoV3)
	if err != nil {
		return nil, err
	}
	token, err := EncryptLegacyGoToken(plaintext, key)
	if err != nil {
		return nil, err
	}
	out := make([]byte, 0, len(EncMagicV3)+SaltLen+len(token))
	out = append(out, EncMagicV3...)
	out = append(out, salt...)
	out = append(out, token...)
	return out, nil
}

// IsLegacyGoEnvelope reports whether raw begins with a known encryption header
// followed by the accidental raw binary token format (byte 0x80 instead of base64).
func IsLegacyGoEnvelope(raw []byte) bool {
	ver, ok := DetectVersion(raw)
	if !ok {
		return false
	}
	var offset int
	switch ver {
	case 1:
		offset = len(EncHeaderV1)
	case 2:
		offset = len(EncMagicV2) + SaltLen
	case 3:
		offset = len(EncMagicV3) + SaltLen
	default:
		return false
	}
	if len(raw) < offset+69 {
		return false
	}
	return raw[offset] == 0x80
}

// atomicWriteFile writes data to a temporary file next to path, syncs it,
// and renames it atomically to path.
func atomicWriteFile(path string, data []byte, perm os.FileMode) error {
	dir := filepath.Dir(path)
	f, err := os.CreateTemp(dir, ".symeraseme_write_*.tmp")
	if err != nil {
		return err
	}
	tmpName := f.Name()
	cleanup := true
	defer func() {
		if cleanup {
			_ = f.Close()
			_ = os.Remove(tmpName)
		}
	}()
	if _, err := f.Write(data); err != nil {
		return err
	}
	if err := os.Chmod(tmpName, perm); err != nil {
		return err
	}
	// Sync the complete, permissioned replacement before it can replace
	// the durable original. The rename is then atomic within this directory.
	if err := f.Sync(); err != nil {
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	if err := replaceFileFn(tmpName, path); err != nil {
		return err
	}
	// A synced file plus an unsynced directory is not crash durable: after
	// a power loss the new directory entry can still disappear. Keep the
	// recovery source untouched by reporting this failure to the caller.
	if err := syncDirectoryFn(dir); err != nil {
		return err
	}
	cleanup = false
	return nil
}

// These hooks keep close and atomic-write failure paths deterministic in tests.
// Production uses the concrete implementations directly.
var replaceFileFn = replaceFile
var atomicWriteFileFn = atomicWriteFile
var syncDirectoryFn = syncDirectory
var removeWALSiblingsFn = RemoveWALSiblings
var closeStoreDBFn = func(s *Store) error { return s.db.Close() }
var checkpointWALFn = func(s *Store) error { return s.CheckpointWAL() }

func cleanupEncryptedTemp(tmpPath string, extraCleanupPaths ...string) error {
	// Remove sidecars first so a failure never discards the recoverable
	// plaintext temp before all WAL/shm cleanup has completed.
	cleanupPaths := append([]string{tmpPath}, extraCleanupPaths...)
	for _, path := range cleanupPaths {
		if path == "" {
			continue
		}
		if err := removeWALSiblingsFn(path); err != nil {
			return err
		}
	}
	if err := os.Remove(tmpPath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return nil
}

// --------------------------------------------------------------------
// At-rest helpers: decrypt-to-temp / encrypt-from-temp
// --------------------------------------------------------------------

// encryptedTempRegistration tracks a decrypted temp and, when available,
// the open Store that owns it. The owner lets FinaliseAll checkpoint and
// close the database before reading its temp file.
type encryptedTempRegistration struct {
	tmpPath     string
	store       *Store
	cleanupPath string // optional original path whose WAL siblings also need cleanup
}

var encryptedTemps sync.Map // map[string]encryptedTempRegistration (origPath → registration)

// RegisterTemp remembers an orig → tmp path for an already-closed database.
// OpenEncrypted uses the private owner-aware registration below so
// FinaliseAll can safely checkpoint and close active stores.
func RegisterTemp(origPath, tmpPath string) {
	encryptedTemps.Store(origPath, encryptedTempRegistration{tmpPath: tmpPath})
}

func registerTempForStoreWithCleanup(origPath, tmpPath string, store *Store, cleanupPath string) {
	encryptedTemps.Store(origPath, encryptedTempRegistration{
		tmpPath: tmpPath, store: store, cleanupPath: cleanupPath,
	})
}

func tempRegistration(value any) (encryptedTempRegistration, bool) {
	reg, ok := value.(encryptedTempRegistration)
	if ok {
		return reg, reg.tmpPath != ""
	}
	// Keep the helper tolerant of registrations made by older in-package
	// callers while all new registrations use the owner-aware form.
	path, ok := value.(string)
	return encryptedTempRegistration{tmpPath: path}, ok && path != ""
}

// ForgetTemp clears the bookkeeping entry.
func ForgetTemp(origPath string) {
	encryptedTemps.Delete(origPath)
}

// DecryptToTemp decrypts srcPath to a freshly-created temp file
// (mode 0600) and registers the pair. Returns the temp file path.
func DecryptToTemp(srcPath, tmpDir string) (string, error) {
	raw, err := os.ReadFile(srcPath) //nolint:gosec // caller path
	if err != nil {
		return "", err
	}
	plain, err := DecryptBytes(raw)
	if err != nil {
		return "", err
	}
	if err := ensurePrivateDir(tmpDir); err != nil {
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
	if err := ensurePrivateFile(tmpPath, 0o600); err != nil {
		return "", err
	}
	RegisterTemp(srcPath, tmpPath)
	return tmpPath, nil
}

// WriteEncrypted re-encrypts the temp file (if changed) back to the
// original path. For an OpenEncrypted registration it first checkpoints
// and closes the owning Store; RegisterTemp callers must provide an
// already-closed database. Always writes a V3 envelope with a fresh random
// salt using standard Fernet. Removes the temp file and WAL siblings
// only after the replacement is durably written; failures leave the
// registered temp state intact for recovery.
func WriteEncrypted(origPath string) error {
	v, ok := encryptedTemps.Load(origPath)
	if !ok {
		return nil
	}
	reg, ok := tempRegistration(v)
	if !ok {
		return errors.New("eventstore: invalid encrypted temp registration")
	}
	if reg.store != nil {
		// OpenEncrypted registrations retain their owner so this public
		// helper cannot read a temp file while SQLite still has WAL state.
		// CloseAt performs the actual write without calling back here.
		return reg.store.CloseAt(origPath)
	}
	tmpPath := reg.tmpPath
	plain, err := os.ReadFile(tmpPath) //nolint:gosec // our own temp file
	if err != nil {
		// Keep the temp registered: it is the recoverable source of truth.
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
	if err := atomicWriteFileFn(origPath, out, 0o600); err != nil {
		return err
	}
	if err := cleanupEncryptedTemp(tmpPath, reg.cleanupPath); err != nil {
		// The replacement is valid, but retain the registration and report
		// cleanup failure so a later finalisation can retry safely.
		return err
	}
	encryptedTemps.Delete(origPath)
	return nil
}

// MigrateV1ToV3 re-encrypts a V1 file to V3 in place atomically.
func MigrateV1ToV3(path string) error {
	return migrateToV3(path)
}

// MigrateV2ToV3 re-encrypts a V2 file to V3 in place atomically.
func MigrateV2ToV3(path string) error {
	return migrateToV3(path)
}

// MigrateLegacyToStandardV3 safely re-encrypts an accidental Go V3 payload to standard Fernet V3.
func MigrateLegacyToStandardV3(path string) error {
	return migrateToV3(path)
}

func migrateToV3(path string) error {
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
	return atomicTransitionFile(path, out, raw)
}

// makePrivateCopy writes a restrictive, synced copy next to path and
// returns its name. The caller owns cleanup of the returned file.
func makePrivateCopy(path string, data []byte, pattern string) (string, error) {
	f, err := os.CreateTemp(filepath.Dir(path), pattern)
	if err != nil {
		return "", err
	}
	copyPath := f.Name()
	keep := false
	defer func() {
		if !keep {
			_ = f.Close()
			_ = os.Remove(copyPath)
		}
	}()
	if err := f.Chmod(0o600); err != nil {
		return "", err
	}
	if _, err := f.Write(data); err != nil {
		return "", err
	}
	if err := f.Sync(); err != nil {
		return "", err
	}
	if err := f.Close(); err != nil {
		return "", err
	}
	keep = true
	return copyPath, nil
}

func makeRecoveryCopy(path string, data []byte) (string, error) {
	return makePrivateCopy(path, data, ".symeraseme_recovery_*.db")
}

func makeTransitionBackup(path string, data []byte) (string, error) {
	return makePrivateCopy(path, data, ".symeraseme_previous_*.db")
}

// EncryptExisting encrypts an existing plaintext file in place
// using V3 (fresh random salt, standard Fernet).
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
	return atomicTransitionFile(path, out, plain)
}

// nowSeconds returns the current Unix time in seconds. Overridable
// from tests to make Fernet tokens deterministic.
var nowSeconds = defaultNowSeconds

type directorySyncFailure struct{ err error }

func (e directorySyncFailure) Error() string { return e.err.Error() }
func (e directorySyncFailure) Unwrap() error { return e.err }

// atomicTransitionFile replaces path while retaining a restrictive recovery
// copy. Replacement failures restore the previous canonical bytes and retain
// the intended replacement; a post-rename directory-sync failure retains the
// new canonical bytes and swaps the recovery copy to the prior source.
func atomicTransitionFile(path string, data, _ []byte) error {
	_, err := atomicTransitionWithRecovery(path, data)
	return err
}

// atomicTransitionWithRecovery returns the retained recovery path on failure.
func atomicTransitionWithRecovery(path string, data []byte) (string, error) {
	previous, readErr := os.ReadFile(path) //nolint:gosec // caller path
	existed := readErr == nil
	if readErr != nil && !errors.Is(readErr, os.ErrNotExist) {
		return "", readErr
	}
	recoveryPath, err := makeRecoveryCopy(path, data)
	if err != nil {
		return "", err
	}
	backupPath := ""
	if existed {
		backupPath, err = makeTransitionBackup(path, previous)
		if err != nil {
			_ = os.Remove(recoveryPath)
			return "", err
		}
	}
	if err := atomicWriteFileFn(path, data, 0o600); err != nil {
		var syncErr directorySyncFailure
		if errors.As(err, &syncErr) && existed {
			// The rename already happened. Retain the new canonical file and
			// expose the prior valid source through the recovery registration.
			_ = os.Remove(recoveryPath)
			if renameErr := os.Rename(backupPath, recoveryPath); renameErr != nil {
				return recoveryPath, errors.Join(err, fmt.Errorf("eventstore: retain previous file: %w", renameErr))
			}
			return recoveryPath, err
		}
		if existed {
			if restoreErr := os.Rename(backupPath, path); restoreErr != nil {
				return recoveryPath, errors.Join(err, fmt.Errorf("eventstore: restore previous file: %w", restoreErr))
			}
		} else if removeErr := os.Remove(path); removeErr != nil && !errors.Is(removeErr, os.ErrNotExist) {
			return recoveryPath, errors.Join(err, fmt.Errorf("eventstore: remove failed replacement: %w", removeErr))
		}
		return recoveryPath, err
	}
	if backupPath != "" {
		if err := os.Remove(backupPath); err != nil && !errors.Is(err, os.ErrNotExist) {
			return recoveryPath, err
		}
	}
	if err := os.Remove(recoveryPath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return recoveryPath, err
	}
	return "", nil
}

// --------------------------------------------------------------------
// OpenEncrypted: full at-rest plumbing
// --------------------------------------------------------------------

// OpenConfigured opens the database using the requested at-rest mode.
// It is the single production entry point used by CLI and MCP call paths.
// Switching from plaintext to encryption, or back, is atomic: a failed
// conversion leaves a restrictive recovery copy when replacement durability
// is ambiguous.
func OpenConfigured(path, tmpDir string, encrypt bool) (*Store, error) {
	if strings.TrimSpace(path) == "" {
		return nil, errors.New("eventstore: database path must not be empty")
	}
	if tmpDir == "" {
		tmpDir = filepath.Join(os.TempDir(), "symeraseme-db")
	}
	if encrypt {
		// Fail before opening a new/plaintext database when encryption was
		// requested but the identity key is unavailable. Otherwise callers
		// could successfully perform work and silently ignore the deferred
		// encrypt-on-close error, leaving sensitive data in plaintext.
		if _, err := currentMasterKey(); err != nil {
			return nil, fmt.Errorf("eventstore: encryption requested: %w", err)
		}
		return OpenEncrypted(path, tmpDir)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, err
	}
	// Plain production opens hold the same sidecar lock for their full
	// lifetime as encrypted opens. This prevents another process from changing
	// the at-rest mode while SQLite still has writable handles or WAL frames.
	lock, err := LockDB(path, 1)
	if err != nil {
		return nil, err
	}
	fail := func(err error) (*Store, error) {
		_ = lock.Close()
		return nil, err
	}
	raw, err := os.ReadFile(path) //nolint:gosec // caller path
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fail(err)
	}
	if _, encrypted := DetectVersion(raw); encrypted {
		if err := DecryptExisting(path); err != nil {
			return fail(err)
		}
	}
	store, err := Open(path)
	if err != nil {
		return fail(err)
	}
	store.dbLock = lock
	return store, nil
}

// DecryptExisting atomically replaces an encrypted database with its
// plaintext contents. Decryption is completed and authenticated before the
// replacement is attempted, so malformed or unauthenticated input cannot
// destroy the source file.
func DecryptExisting(path string) error {
	raw, err := os.ReadFile(path) //nolint:gosec // caller path
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	if len(raw) == 0 {
		return nil
	}
	if _, encrypted := DetectVersion(raw); !encrypted {
		return nil
	}
	plain, err := DecryptBytes(raw)
	if err != nil {
		return err
	}
	return atomicTransitionFile(path, plain, raw)
}

// OpenEncrypted always keeps the canonical path encrypted while the store is
// in use. SQLite is opened only on a private decrypted temp file, and the
// exclusive lock remains held until the encrypted close sequence succeeds.
func OpenEncrypted(encPath, tmpDir string) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(encPath), 0o700); err != nil {
		return nil, err
	}
	if err := ensurePrivateDir(tmpDir); err != nil {
		return nil, err
	}
	lock, err := LockDB(encPath, 1)
	if err != nil {
		return nil, err
	}
	fail := func(err error) (*Store, error) {
		_ = lock.Close()
		return nil, err
	}
	raw, err := os.ReadFile(encPath) //nolint:gosec // caller path
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fail(err)
	}
	if errors.Is(err, os.ErrNotExist) || len(raw) == 0 {
		// Initialize SQLite away from the canonical path, checkpoint it, and
		// atomically publish its encrypted form before returning.
		initPath, err := newPrivateDBPath(tmpDir)
		if err != nil {
			return fail(err)
		}
		initStore, err := Open(initPath)
		if err != nil {
			_ = os.Remove(initPath)
			return fail(err)
		}
		if err := initStore.Close(); err != nil {
			_ = os.Remove(initPath)
			return fail(err)
		}
		if err := EncryptExisting(initPath); err != nil {
			return fail(err)
		}
		initialCiphertext, err := os.ReadFile(initPath) //nolint:gosec // private initializer
		if err != nil {
			return fail(err)
		}
		if err := atomicWriteFileFn(encPath, initialCiphertext, 0o600); err != nil {
			return fail(err)
		}
		if err := cleanupEncryptedTemp(initPath); err != nil {
			return fail(err)
		}
		raw = initialCiphertext
	}
	version, ok := DetectVersion(raw)
	if !ok {
		// Checkpoint an existing plaintext database before reading it for
		// encryption; otherwise committed frames still in its WAL could be
		// lost by an in-place file transition.
		plainStore, openErr := Open(encPath)
		if openErr != nil {
			return fail(openErr)
		}
		if checkpointErr := checkpointWALFn(plainStore); checkpointErr != nil {
			_ = plainStore.db.Close()
			return fail(checkpointErr)
		}
		if closeErr := closeStoreDBFn(plainStore); closeErr != nil {
			_ = plainStore.db.Close()
			return fail(closeErr)
		}
		plainStore.dbClosed = true
		// Existing plaintext is converted before the encrypted temp copy is
		// opened. The restrictive recovery copy remains if replacement or
		// directory sync is ambiguous.
		if err := EncryptExisting(encPath); err != nil {
			return fail(err)
		}
		raw, err = os.ReadFile(encPath) //nolint:gosec // caller path
		if err != nil {
			return fail(err)
		}
		version, ok = DetectVersion(raw)
		if !ok {
			return fail(errors.New("eventstore: encrypted conversion did not produce an encrypted file"))
		}
	}
	// Encrypted — transparently migrate to V3 while the lock is held, then
	// decrypt only to the private temp directory.
	switch version {
	case 1:
		if err := MigrateV1ToV3(encPath); err != nil {
			return fail(err)
		}
	case 2:
		if err := MigrateV2ToV3(encPath); err != nil {
			return fail(err)
		}
	case 3:
		if IsLegacyGoEnvelope(raw) {
			if err := MigrateLegacyToStandardV3(encPath); err != nil {
				return fail(err)
			}
		}
	}
	tmpPath, err := DecryptToTemp(encPath, tmpDir)
	if err != nil {
		return fail(err)
	}
	s, err := Open(tmpPath)
	if err != nil {
		return fail(err)
	}
	// Callers see the canonical path, but all SQLite I/O is on tmpPath.
	s.path = encPath
	s.encryptedPath = encPath
	s.dbLock = lock
	registerTempForStoreWithCleanup(encPath, tmpPath, s, encPath)
	return s, nil
}

func newPrivateDBPath(tmpDir string) (string, error) {
	f, err := os.CreateTemp(tmpDir, "symeraseme_init_*.db")
	if err != nil {
		return "", err
	}
	path := f.Name()
	if err := f.Close(); err != nil {
		_ = os.Remove(path)
		return "", err
	}
	if err := ensurePrivateFile(path, 0o600); err != nil {
		_ = os.Remove(path)
		return "", err
	}
	return path, nil
}

// CloseAt closes the Store, checkpoints the WAL, and re-encrypts the temp file back to
// the original path (if a temp was registered). The standard Fernet V3 envelope is
// always used on the way out. It is also the implementation used by Close for
// stores returned from OpenEncrypted; the lock is kept in the Store so the two
// APIs cannot recurse or double-close.
func (s *Store) CloseAt(encPath string) error {
	if s == nil {
		return nil
	}
	s.closeMu.Lock()
	defer s.closeMu.Unlock()
	return s.closeAtLocked(encPath)
}

func (s *Store) closeAtLocked(encPath string) error {
	if !s.dbClosed {
		// A failed checkpoint means the main database may not contain the
		// latest WAL frames. Keep the database open and its registration
		// untouched so a later Close/FinaliseAll can retry the checkpoint.
		if err := checkpointWALFn(s); err != nil {
			return err
		}
		if err := closeStoreDBFn(s); err != nil {
			return err
		}
		s.dbClosed = true
	}

	v, ok := encryptedTemps.Load(encPath)
	if !ok {
		s.encryptedPath = ""
		return nil
	}
	reg, ok := tempRegistration(v)
	if !ok {
		return errors.New("eventstore: invalid encrypted temp registration")
	}
	tmpPath := reg.tmpPath
	if tmpPath == encPath {
		// Preserve a recoverable plaintext source while encrypting in place.
		// If replacement or directory sync fails, atomicTransitionFile restores
		// the canonical source and retains the intended encrypted replacement.
		plain, err := os.ReadFile(encPath) //nolint:gosec // caller path
		if err != nil {
			return err
		}
		sourcePath, err := makeTransitionBackup(encPath, plain)
		if err != nil {
			return err
		}
		if err := EncryptExisting(encPath); err != nil {
			registerTempForStoreWithCleanup(encPath, encPath, s, encPath)
			return err
		}
		if err := removeWALSiblingsFn(encPath); err != nil {
			registerTempForStoreWithCleanup(encPath, sourcePath, s, encPath)
			return err
		}
		if err := os.Remove(sourcePath); err != nil && !errors.Is(err, os.ErrNotExist) {
			registerTempForStoreWithCleanup(encPath, sourcePath, s, encPath)
			return err
		}
		encryptedTemps.Delete(encPath)
		s.encryptedPath = ""
		return s.releaseDBLock()
	}

	// Re-encrypt tmp → encPath. Every failure leaves the temp and its WAL
	// siblings registered and untouched for recovery.
	plain, err := os.ReadFile(tmpPath) //nolint:gosec // our own temp file
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
	if err := atomicTransitionFile(encPath, out, nil); err != nil {
		return err
	}
	if err := cleanupEncryptedTemp(tmpPath, reg.cleanupPath); err != nil {
		return err
	}
	encryptedTemps.Delete(encPath)
	s.encryptedPath = ""
	return s.releaseDBLock()
}

func (s *Store) releaseDBLock() error {
	if s == nil || s.dbLock == nil {
		return nil
	}
	if err := s.dbLock.Close(); err != nil {
		return err
	}
	s.dbLock = nil
	return nil
}
