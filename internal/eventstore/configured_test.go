package eventstore

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestOpenConfiguredPlainEncryptedTransitionsPersistData(t *testing.T) {
	master := bytes.Repeat([]byte{0x4a}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	tmpDir := filepath.Join(dir, "encrypted-temp")
	ctx := context.Background()

	plain, err := OpenConfigured(path, tmpDir, false)
	if err != nil {
		t.Fatalf("open plain = %v", err)
	}
	requestID, err := plain.CreateRemovalRequest(ctx, "broker", "email", "campaign", "DE", "gdpr-art17", "hash")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := plain.Append(ctx, requestID, EvtSent, map[string]any{"expected_response_days": 4}, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	if err := plain.Close(); err != nil {
		t.Fatalf("close plain = %v", err)
	}

	encrypted, err := OpenConfigured(path, tmpDir, true)
	if err != nil {
		t.Fatalf("open encrypted = %v", err)
	}
	if encrypted.Path() != path {
		t.Fatalf("encrypted Path() = %q, want %q", encrypted.Path(), path)
	}
	if err := encrypted.Close(); err != nil {
		t.Fatalf("encrypt close = %v", err)
	}
	if encryptedBytes, err := os.ReadFile(path); err != nil || !bytes.HasPrefix(encryptedBytes, EncMagicV3) {
		t.Fatalf("encrypted file prefix = %q, err=%v", encryptedBytes[:min(len(encryptedBytes), len(EncMagicV3))], err)
	}

	// Turning encryption off must decrypt atomically and keep the same data.
	decrypted, err := OpenConfigured(path, tmpDir, false)
	if err != nil {
		t.Fatalf("open after disabling encryption = %v", err)
	}
	if err := decrypted.Close(); err != nil {
		t.Fatalf("close decrypted = %v", err)
	}
	plainBytes, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.HasPrefix(plainBytes, []byte("SYMERASEME_ENC")) {
		t.Fatalf("encryption-disabled file still has encrypted header: %q", plainBytes[:min(len(plainBytes), 20)])
	}

	reopened, err := OpenConfigured(path, tmpDir, false)
	if err != nil {
		t.Fatalf("restart reopen = %v", err)
	}
	state, err := reopened.RebuildState(ctx, requestID)
	if err != nil {
		t.Fatalf("restart rebuild = %v", err)
	}
	if state.CurrentStatus != "AWAITING_ACK" {
		t.Fatalf("restart status = %q, want AWAITING_ACK", state.CurrentStatus)
	}
	if err := reopened.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestOpenConfiguredRejectsMalformedEncryptedFileWithoutMutation(t *testing.T) {
	master := bytes.Repeat([]byte{0x51}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	original := append([]byte(nil), EncMagicV3...)
	original = append(original, bytes.Repeat([]byte{0x01}, SaltLen)...)
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenConfigured(path, filepath.Join(dir, "tmp"), false); err == nil {
		t.Fatal("truncated encrypted file was accepted")
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, original) {
		t.Fatal("malformed encrypted file was modified")
	}
}

func TestOpenConfiguredPreservesPlainFileOnFailedEncryptionTransition(t *testing.T) {
	master := bytes.Repeat([]byte{0x61}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	plain, err := OpenConfigured(path, filepath.Join(dir, "tmp"), false)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := plain.CreateRemovalRequest(context.Background(), "broker", "email", "campaign", "US", "", ""); err != nil {
		t.Fatal(err)
	}
	if err := plain.Close(); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	atomicWriteFileFn = func(string, []byte, os.FileMode) error { return errors.New("injected replacement failure") }
	if _, err := OpenConfigured(path, filepath.Join(dir, "tmp"), true); err == nil || !containsError(err, "injected replacement failure") {
		t.Fatalf("encryption transition error = %v", err)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, before) || bytes.HasPrefix(got, EncMagicV3) {
		t.Fatal("failed encryption transition replaced the plaintext source")
	}
}

func containsError(err error, want string) bool {
	return err != nil && bytes.Contains([]byte(err.Error()), []byte(want))
}

func TestOpenConfiguredRejectsEncryptionWithoutMasterKey(t *testing.T) {
	SetMasterKeyProvider(nil)
	path := filepath.Join(t.TempDir(), DBFileName)
	if _, err := OpenConfigured(path, filepath.Join(t.TempDir(), "tmp"), true); err == nil {
		t.Fatal("encryption without an identity master key must fail before opening")
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("failed encrypted open created database: %v", err)
	}
}

func TestOpenEncryptedPublishesCiphertextBeforeReturning(t *testing.T) {
	master := bytes.Repeat([]byte{0x7a}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	plain, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := plain.CreateCampaign(context.Background(), "before-encrypt", "initial", ""); err != nil {
		t.Fatal(err)
	}
	if err := plain.Close(); err != nil {
		t.Fatal(err)
	}

	store, err := OpenEncrypted(path, filepath.Join(dir, "private-temp"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(raw, EncMagicV3) {
		t.Fatalf("canonical database is not ciphertext while open: %q", raw[:min(len(raw), len(EncMagicV3))])
	}
	if store.Path() != path {
		t.Fatalf("store path = %q, want canonical path %q", store.Path(), path)
	}
	regValue, ok := encryptedTemps.Load(path)
	if !ok {
		t.Fatal("encrypted store did not register private temp")
	}
	reg, ok := tempRegistration(regValue)
	if !ok || reg.tmpPath == path || !strings.Contains(reg.tmpPath, "private-temp") {
		t.Fatalf("registration = %+v, want private decrypted temp", reg)
	}
}

func TestOpenEncryptedExcludesSecondStoreUntilClose(t *testing.T) {
	master := bytes.Repeat([]byte{0x7b}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	first, err := OpenEncrypted(path, filepath.Join(dir, "tmp-one"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := OpenEncrypted(path, filepath.Join(dir, "tmp-two")); err == nil {
		t.Fatal("second encrypted opener acquired the database lock")
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	second, err := OpenEncrypted(path, filepath.Join(dir, "tmp-three"))
	if err != nil {
		t.Fatal(err)
	}
	if err := second.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestEncryptionTransitionFailureLeavesRecoveryCopy(t *testing.T) {
	master := bytes.Repeat([]byte{0x7c}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	plain, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := plain.CreateCampaign(context.Background(), "recovery", "initial", ""); err != nil {
		t.Fatal(err)
	}
	if err := plain.Close(); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	syncErr := errors.New("transition directory sync failure")
	syncDirectoryFn = func(string) error { return syncErr }
	if _, err := OpenEncrypted(path, filepath.Join(dir, "transition-temp")); !errors.Is(err, syncErr) {
		t.Fatalf("OpenEncrypted error = %v, want %v", err, syncErr)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(got, EncMagicV3) {
		t.Fatalf("post-rename sync failure did not leave authenticated ciphertext: %q", got[:min(len(got), len(EncMagicV3))])
	}
	plainReplacement, err := DecryptBytes(got)
	if err != nil || !bytes.Equal(plainReplacement, before) {
		t.Fatalf("canonical replacement lost plaintext: decrypt=%v", err)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".symeraseme_recovery_") || strings.HasPrefix(entry.Name(), ".symeraseme_previous_") {
			t.Fatalf("standalone transition left recovery artifact %s", entry.Name())
		}
	}
}

func TestConfiguredPlainOpenBlocksModeTransitionUntilClose(t *testing.T) {
	master := bytes.Repeat([]byte{0x6d}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	tmpDir := filepath.Join(dir, "encrypted-temp")

	plain, err := OpenConfigured(path, tmpDir, false)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := plain.CreateCampaign(context.Background(), "before-transition", "initial", ""); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenConfigured(path, tmpDir, true); err == nil {
		t.Fatal("mode transition acquired the lock while plaintext store was open")
	}
	if _, err := plain.CreateCampaign(context.Background(), "after-blocked-transition", "initial", ""); err != nil {
		t.Fatalf("plaintext writer failed after blocked transition: %v", err)
	}
	if err := plain.Close(); err != nil {
		t.Fatal(err)
	}

	encrypted, err := OpenConfigured(path, tmpDir, true)
	if err != nil {
		t.Fatalf("mode transition after plaintext close: %v", err)
	}
	for _, campaignID := range []string{"before-transition", "after-blocked-transition"} {
		created, err := encrypted.CreateCampaign(context.Background(), campaignID, "initial", "")
		if err != nil {
			t.Fatal(err)
		}
		if created {
			t.Fatalf("campaign %q was lost during mode transition", campaignID)
		}
	}
	if err := encrypted.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestCloseAtRetriesAfterPostRenameSyncFailureUsesPlaintextSource(t *testing.T) {
	master := bytes.Repeat([]byte{0x81}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	path := filepath.Join(dir, "retry-post-rename.db")
	plain := []byte("sqlite plaintext that must not be encrypted twice")
	if err := os.WriteFile(path, plain, 0o600); err != nil {
		t.Fatal(err)
	}
	RegisterTemp(path, path)
	t.Cleanup(func() { ForgetTemp(path) })
	store := openCheckpointStore(t, "eventstore_test_checkpoint_ok")

	syncErr := errors.New("post-rename directory sync failure")
	syncCalls := 0
	syncDirectoryFn = func(string) error {
		syncCalls++
		if syncCalls == 1 {
			return syncErr
		}
		return nil
	}
	if err := store.CloseAt(path); !errors.Is(err, syncErr) {
		t.Fatalf("first CloseAt = %v, want %v", err, syncErr)
	}
	regValue, ok := encryptedTemps.Load(path)
	if !ok {
		t.Fatal("failed transition lost its retry registration")
	}
	reg, ok := tempRegistration(regValue)
	if !ok || reg.tmpPath == path {
		t.Fatalf("retry registration = %+v, want a private plaintext source", reg)
	}
	registeredPlain, err := os.ReadFile(reg.tmpPath)
	if err != nil || !bytes.Equal(registeredPlain, plain) {
		t.Fatalf("retry source = %q, err=%v", registeredPlain, err)
	}
	canonical, err := os.ReadFile(path)
	if err != nil || !bytes.HasPrefix(canonical, EncMagicV3) {
		t.Fatalf("canonical after post-rename failure = %q, err=%v", canonical[:min(len(canonical), len(EncMagicV3))], err)
	}

	if err := store.CloseAt(path); err != nil {
		t.Fatalf("retry CloseAt = %v", err)
	}
	if _, ok := encryptedTemps.Load(path); ok {
		t.Fatal("successful retry left a transition registration")
	}
	got, err := DecryptFile(path)
	if err != nil || !bytes.Equal(got, plain) {
		t.Fatalf("decrypted retry result = %q, err=%v", got, err)
	}
}

func TestDecryptTransitionFailureCleansPlaintextRecovery(t *testing.T) {
	master := bytes.Repeat([]byte{0x82}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	path := filepath.Join(dir, "decrypt-failure.db")
	plain := []byte("authenticated plaintext")
	if err := os.WriteFile(path, plain, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := EncryptExisting(path); err != nil {
		t.Fatal(err)
	}

	syncErr := errors.New("decrypt directory sync failure")
	syncDirectoryFn = func(string) error { return syncErr }
	if err := DecryptExisting(path); !errors.Is(err, syncErr) {
		t.Fatalf("DecryptExisting = %v, want %v", err, syncErr)
	}
	got, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(got, plain) {
		t.Fatalf("canonical after failed decrypt = %q, err=%v", got, err)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".symeraseme_recovery_") || strings.HasPrefix(entry.Name(), ".symeraseme_previous_") {
			t.Fatalf("failed decrypt left recovery artifact %s", entry.Name())
		}
	}
}

func TestOpenEncryptedInitializerFailureCleansAllArtifacts(t *testing.T) {
	master := bytes.Repeat([]byte{0x83}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	restoreCloseHooks(t)
	dir := t.TempDir()
	tmpDir := filepath.Join(dir, "private-temp")
	atomicWriteFileFn = func(string, []byte, os.FileMode) error {
		return errors.New("initializer encryption failure")
	}
	if _, err := OpenEncrypted(filepath.Join(dir, "new.db"), tmpDir); err == nil || !strings.Contains(err.Error(), "initializer encryption failure") {
		t.Fatalf("OpenEncrypted = %v, want initializer failure", err)
	}
	entries, err := os.ReadDir(tmpDir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "symeraseme_init_") || strings.HasPrefix(entry.Name(), ".symeraseme_recovery_") || strings.HasPrefix(entry.Name(), ".symeraseme_previous_") {
			t.Fatalf("initializer failure left artifact %s", entry.Name())
		}
	}
}

func TestEncryptedOpenBlocksCrossModePlainOpenUntilClose(t *testing.T) {
	master := bytes.Repeat([]byte{0x84}, 32)
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })
	dir := t.TempDir()
	path := filepath.Join(dir, DBFileName)
	tmpDir := filepath.Join(dir, "private-temp")
	plain, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := plain.CreateCampaign(context.Background(), "cross-mode", "initial", ""); err != nil {
		t.Fatal(err)
	}
	if err := plain.Close(); err != nil {
		t.Fatal(err)
	}

	encrypted, err := OpenConfigured(path, tmpDir, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := OpenConfigured(path, tmpDir, false); err == nil {
		t.Fatal("plain opener acquired the lock while encrypted store was open")
	}
	if err := encrypted.Close(); err != nil {
		t.Fatal(err)
	}
	plainAgain, err := OpenConfigured(path, tmpDir, false)
	if err != nil {
		t.Fatalf("plain opener after encrypted close = %v", err)
	}
	if err := plainAgain.Close(); err != nil {
		t.Fatal(err)
	}
}
