package eventstore

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func fixedTestMaster() []byte { return bytes.Repeat([]byte{0x42}, 32) }

func TestFernetAndEnvelopeValidation(t *testing.T) {
	master := fixedTestMaster()
	plaintext := []byte("eventstore payload")
	key := bytes.Repeat([]byte{0x24}, 32)
	token, err := EncryptFernetToken(plaintext, key)
	if err != nil {
		t.Fatal(err)
	}
	decrypted, err := DecryptFernetToken(token, key)
	if err != nil || !bytes.Equal(decrypted, plaintext) {
		t.Fatalf("Fernet round trip = %q, err=%v", decrypted, err)
	}
	badMAC := append([]byte(nil), token...)
	badMAC[len(badMAC)-1] ^= 1
	if _, err := DecryptFernetToken(badMAC, key); err == nil {
		t.Fatal("tampered HMAC accepted")
	}
	badVersion := append([]byte(nil), token...)
	badVersion[0] = 1
	if _, err := DecryptFernetToken(badVersion, key); err == nil {
		t.Fatal("unsupported Fernet version accepted")
	}
	if _, err := DecryptFernetToken(token[:10], key); err == nil {
		t.Fatal("short Fernet token accepted")
	}
	if _, err := DecryptFernetToken(token, []byte("short")); err == nil {
		t.Fatal("invalid Fernet key length accepted")
	}
	if _, err := EncryptFernetToken(plaintext, []byte("short")); err == nil {
		t.Fatal("invalid encryption key length accepted")
	}

	v2, err := EncryptBytesV2(plaintext, master)
	if err != nil {
		t.Fatal(err)
	}
	v3, err := EncryptBytesV3(plaintext, master)
	if err != nil {
		t.Fatal(err)
	}
	if version, ok := DetectVersion(v2); !ok || version != 2 {
		t.Fatalf("V2 detection = %d, %v", version, ok)
	}
	if version, ok := DetectVersion(v3); !ok || version != 3 {
		t.Fatalf("V3 detection = %d, %v", version, ok)
	}
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	for name, envelope := range map[string][]byte{"v2": v2, "v3": v3} {
		got, err := DecryptBytes(envelope)
		if err != nil || !bytes.Equal(got, plaintext) {
			t.Fatalf("%s decrypt = %q, err=%v", name, got, err)
		}
	}
	if _, err := DecryptBytes([]byte("plain")); !errors.Is(err, ErrUnrecognizedHeader) {
		t.Fatalf("plain decrypt error = %v", err)
	}
	if _, err := DecryptBytes(v3[:len(EncMagicV3)]); err == nil {
		t.Fatal("truncated V3 envelope accepted")
	}
}

func TestEncryptedFileHelpersAndTempRegistration(t *testing.T) {
	master := fixedTestMaster()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	plainPath := filepath.Join(dir, "plain.db")
	if err := os.WriteFile(plainPath, []byte("plain data"), 0o600); err != nil {
		t.Fatal(err)
	}
	if encrypted, err := IsEncrypted(plainPath); err != nil || encrypted {
		t.Fatalf("plain IsEncrypted = %v, err=%v", encrypted, err)
	}
	if encrypted, err := IsEncrypted(filepath.Join(dir, "missing")); err != nil || encrypted {
		t.Fatalf("missing IsEncrypted = %v, err=%v", encrypted, err)
	}
	if err := EncryptExisting(plainPath); err != nil {
		t.Fatal(err)
	}
	if encrypted, err := IsEncrypted(plainPath); err != nil || !encrypted {
		t.Fatalf("encrypted IsEncrypted = %v, err=%v", encrypted, err)
	}
	if got, err := DecryptFile(plainPath); err != nil || !bytes.Equal(got, []byte("plain data")) {
		t.Fatalf("DecryptFile = %q, err=%v", got, err)
	}
	tmpDir := filepath.Join(dir, "tmp")
	tmpPath, err := DecryptToTemp(plainPath, tmpDir)
	if err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(tmpPath); err != nil || !bytes.Equal(got, []byte("plain data")) {
		t.Fatalf("temp plaintext = %q, err=%v", got, err)
	}
	if _, err := os.Stat(tmpPath + ".lock"); err != nil {
		t.Fatalf("decrypted temp lock sidecar missing: %v", err)
	}
	if err := WriteEncrypted(plainPath); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(tmpPath); !os.IsNotExist(err) {
		t.Fatalf("temp file remains after WriteEncrypted: %v", err)
	}
	if _, err := os.Stat(tmpPath + ".lock"); !os.IsNotExist(err) {
		t.Fatalf("temp lock sidecar remains after WriteEncrypted: %v", err)
	}
	if err := WriteEncrypted(filepath.Join(dir, "unregistered")); err != nil {
		t.Fatal(err)
	}
}

func TestOpenEncryptedFailureRetainsTempRegistrationOnCleanupFailure(t *testing.T) {
	master := fixedTestMaster()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	encPath := filepath.Join(dir, "invalid.db")
	ciphertext, err := EncryptBytesV3([]byte("not sqlite"), master)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(encPath, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}
	cleanupErr := errors.New("temp cleanup failure")
	oldRemoveWAL := removeWALSiblingsFn
	removeWALSiblingsFn = func(string) error { return cleanupErr }
	t.Cleanup(func() { removeWALSiblingsFn = oldRemoveWAL })

	if _, err := OpenEncrypted(encPath, filepath.Join(dir, "tmp")); err == nil || !errors.Is(err, cleanupErr) {
		t.Fatalf("OpenEncrypted error = %v, want cleanup failure", err)
	}
	value, ok := encryptedTemps.Load(encPath)
	if !ok {
		t.Fatal("failed open discarded retry registration")
	}
	reg, ok := tempRegistration(value)
	if !ok || reg.tempLock == nil {
		t.Fatalf("retry registration = %+v, want held temp lock", reg)
	}
	if _, err := os.Stat(reg.tmpPath); err != nil {
		t.Fatalf("retry plaintext temp missing: %v", err)
	}
	if lock, err := tryLockDB(reg.tmpPath); err == nil {
		_ = lock.Close()
		t.Fatal("retry temp lock was released after cleanup failure")
	}

	removeWALSiblingsFn = RemoveWALSiblings
	if err := cleanupEncryptedTempWithLock(reg.tmpPath, reg.tempLock); err != nil {
		t.Fatalf("retry temp cleanup = %v", err)
	}
	encryptedTemps.Delete(encPath)
}
