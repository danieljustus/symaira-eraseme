package identity

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestRegressionDeletionResurrection verifies:
// 1. Default DeleteProfile removes both identity.encrypted and legacy identity.enc without resurrecting.
// 2. Explicit custom path deletion stays strictly scoped to the specified path.
func TestRegressionDeletionResurrection(t *testing.T) {
	key := testKey(t)
	t.Cleanup(func() { _ = SetMasterKey(nil) })

	// Case 1: Default deletion removes both identity.encrypted and identity.enc
	dir := t.TempDir()
	t.Setenv("HOME", dir)
	t.Setenv("SYMERASEME_DATA_DIR", dir)
	t.Setenv("SYMERASEME_CONFIG_DIR", dir)
	t.Setenv("SYMERASEME_IDENTITY_PATH", "")

	goPath := filepath.Join(dir, "identity.encrypted")
	pyPath := filepath.Join(dir, "identity.enc")

	p := &Profile{FullName: "Resurrection Test", EmailAddresses: []string{"test@example.com"}}
	plain, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	ciphertext, err := encryptProfile(plain, key)
	if err != nil {
		t.Fatal(err)
	}

	// Write both files to disk
	if err := os.WriteFile(goPath, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(pyPath, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}

	// Verify both exist
	if !ProfileExists("") {
		t.Fatal("ProfileExists returned false before deletion")
	}
	if _, err := LoadProfile(""); err != nil {
		t.Fatalf("LoadProfile failed before deletion: %v", err)
	}

	// Perform default deletion
	if err := DeleteProfile(""); err != nil {
		t.Fatalf("DeleteProfile failed: %v", err)
	}

	// Verify BOTH files were removed from disk
	if _, err := os.Stat(goPath); !errors.Is(err, os.ErrNotExist) {
		t.Errorf("identity.encrypted still exists after DeleteProfile: %v", err)
	}
	if _, err := os.Stat(pyPath); !errors.Is(err, os.ErrNotExist) {
		t.Errorf("legacy identity.enc still exists after DeleteProfile: %v", err)
	}

	// Verify profile is NOT resurrected on subsequent calls
	if ProfileExists("") {
		t.Fatal("Profile was resurrected after DeleteProfile!")
	}
	if _, err := LoadProfile(""); !errors.Is(err, ErrProfileNotFound) {
		t.Fatalf("expected ErrProfileNotFound after DeleteProfile, got: %v", err)
	}

	// Case 2: Explicit custom path deletion stays strictly scoped
	customDir := t.TempDir()
	customEncrypted := filepath.Join(customDir, "custom.encrypted")
	customLegacyEnc := filepath.Join(customDir, "custom.enc")

	if err := os.WriteFile(customEncrypted, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(customLegacyEnc, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}

	// Delete only custom.encrypted explicitly
	if err := DeleteProfile(customEncrypted); err != nil {
		t.Fatalf("DeleteProfile(customEncrypted) failed: %v", err)
	}

	// Verify custom.encrypted is gone, but custom.enc is STILL PRESENT
	if _, err := os.Stat(customEncrypted); !errors.Is(err, os.ErrNotExist) {
		t.Errorf("custom.encrypted was not deleted: %v", err)
	}
	if _, err := os.Stat(customLegacyEnc); err != nil {
		t.Errorf("custom.enc was unexpectedly deleted; custom paths must stay scoped: %v", err)
	}
}

// TestRegressionAtomicWriteFailureAndRecovery verifies:
// 1. SaveProfile writes atomically using tempfile, chmod, fsync, rename, and dir fsync.
// 2. Leftover temp files do not corrupt LoadProfile or subsequent writes.
// 3. Write failures do not truncate or corrupt the original profile.
// 4. Recovery writes succeed and overwrite cleanly.
func TestRegressionAtomicWriteFailureAndRecovery(t *testing.T) {
	_ = testKey(t)
	clearKey(t)
	t.Cleanup(func() { _ = SetMasterKey(nil) })

	dir := t.TempDir()
	path := filepath.Join(dir, "identity.encrypted")

	p1 := &Profile{
		FullName:       "Original Profile",
		EmailAddresses: []string{"orig@example.com"},
	}

	// 1. Initial successful atomic write
	savedPath, err := SaveProfile(p1, path)
	if err != nil {
		t.Fatalf("initial SaveProfile failed: %v", err)
	}
	if savedPath != path {
		t.Fatalf("SaveProfile returned %s, want %s", savedPath, path)
	}

	// Check file mode is 0600
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat profile: %v", err)
	}
	if perm := info.Mode().Perm(); perm != ProfileFileMode {
		t.Errorf("expected permissions %o, got %o", ProfileFileMode, perm)
	}

	loaded1, err := LoadProfile(path)
	if err != nil || loaded1.FullName != "Original Profile" {
		t.Fatalf("LoadProfile failed: %v, profile: %+v", err, loaded1)
	}

	// 2. Simulate leftover temp files from a previous interrupted process
	staleTmp := filepath.Join(dir, ".identity.encrypted.tmp-stale-12345")
	if err := os.WriteFile(staleTmp, []byte("garbage corrupt temp data"), 0o600); err != nil {
		t.Fatal(err)
	}

	// LoadProfile must NOT read or be affected by leftover temp file
	loadedStill1, err := LoadProfile(path)
	if err != nil || loadedStill1.FullName != "Original Profile" {
		t.Fatalf("LoadProfile corrupted by stale temp: %v", err)
	}

	// 3. Simulate write failure: make directory unwritable (read-only 0500 on unix)
	p2 := &Profile{
		FullName:       "Updated Profile",
		EmailAddresses: []string{"updated@example.com"},
	}

	if err := os.Chmod(dir, 0o500); err != nil {
		t.Skipf("cannot chmod dir to 0500: %v", err)
	}

	// SaveProfile should fail because temp file cannot be created in dir
	_, writeErr := SaveProfile(p2, path)
	if writeErr == nil {
		_ = os.Chmod(dir, 0o700)
		t.Fatal("expected SaveProfile to fail in read-only directory")
	}

	// Restore dir permissions immediately for assertions and cleanup
	if err := os.Chmod(dir, 0o700); err != nil {
		t.Fatalf("restore chmod dir: %v", err)
	}

	// Original profile at path must NOT be truncated or corrupted!
	loadedOriginal, err := LoadProfile(path)
	if err != nil {
		t.Fatalf("original profile was corrupted during failed write: %v", err)
	}
	if loadedOriginal.FullName != "Original Profile" {
		t.Fatalf("expected Original Profile intact, got: %s", loadedOriginal.FullName)
	}

	// Verify no new temp files leaked
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), ".identity.encrypted.tmp-") && e.Name() != ".identity.encrypted.tmp-stale-12345" {
			t.Errorf("leaked temp file found: %s", e.Name())
		}
	}

	// 4. Recovery write after permissions restored
	if _, err := SaveProfile(p2, path); err != nil {
		t.Fatalf("recovery SaveProfile failed: %v", err)
	}
	loaded2, err := LoadProfile(path)
	if err != nil || loaded2.FullName != "Updated Profile" {
		t.Fatalf("LoadProfile after recovery write failed: %v, profile: %+v", err, loaded2)
	}
}

// TestRegressionInitProfileKeyRollback verifies:
// 1. If profile write fails during InitProfile and key was newly created, the key is rolled back from keychain and cache.
// 2. If master key already existed prior to InitProfile, it is NOT rolled back.
func TestRegressionInitProfileKeyRollback(t *testing.T) {
	fake := NewFakeKeyring()
	SetKeyringBackend(fake)
	t.Cleanup(func() {
		SetKeyringBackend(nil)
		_ = SetMasterKey(nil)
	})

	prof := &Profile{FullName: "Rollback Test", EmailAddresses: []string{"rollback@example.com"}}
	unwritablePath := "/dev/null/impossible_dir/identity.encrypted"

	// Subtest 1: Fresh initialization fails -> key is rolled back
	_ = SetMasterKey(nil)
	t.Setenv(EnvMasterKeyHex, "")
	t.Setenv(EnvSymvaultPassphrase, "")

	// Verify initial state: no key in keyring, no key in cache
	if k, err := fake.Get(ServiceName, KeyringUsername); err == nil && k != "" {
		t.Fatalf("keyring not initially empty: %s", k)
	}
	if _, err := GetExistingMasterKey(); !errors.Is(err, ErrMasterKeyMissing) {
		t.Fatal("cache/env not initially empty")
	}

	// InitProfile with invalid path must fail
	_, err := InitProfile(prof, unwritablePath)
	if err == nil {
		t.Fatal("expected InitProfile to fail for invalid path")
	}

	// Key must have been rolled back from keyring!
	if stored, err := fake.Get(ServiceName, KeyringUsername); err == nil && stored != "" {
		t.Fatalf("newly minted key was NOT rolled back from keyring: %s", stored)
	}

	// In-process cache must also be clear!
	if cached, err := GetExistingMasterKey(); !errors.Is(err, ErrMasterKeyMissing) {
		t.Fatalf("newly minted key was NOT rolled back from cache: %x", cached)
	}

	// Subtest 2: Existing key is NOT rolled back on failure
	existingKey := []byte("0123456789abcdef0123456789abcdef")
	existingHex := hex.EncodeToString(existingKey)
	if err := fake.Set(ServiceName, KeyringUsername, existingHex); err != nil {
		t.Fatal(err)
	}
	if err := SetMasterKey(existingKey); err != nil {
		t.Fatal(err)
	}

	// InitProfile fails due to unwritable path
	_, err = InitProfile(prof, unwritablePath)
	if err == nil {
		t.Fatal("expected InitProfile to fail for invalid path")
	}

	// Pre-existing key must STILL exist in keyring!
	stored, err := fake.Get(ServiceName, KeyringUsername)
	if err != nil || stored != existingHex {
		t.Fatalf("pre-existing key was incorrectly rolled back or altered: stored=%q, want=%q, err=%v", stored, existingHex, err)
	}

	// Pre-existing key must STILL exist in cache!
	cached, err := GetExistingMasterKey()
	if err != nil || string(cached) != string(existingKey) {
		t.Fatalf("pre-existing key was incorrectly cleared from cache: got=%x, want=%x, err=%v", cached, existingKey, err)
	}
}

// TestRegressionCanonicalFieldEvolution verifies that the generic canonical JSON encoder:
// 1. Automatically includes newly added fields without silent omission or schema code changes.
// 2. Preserves strict alphabetical key sorting across all nesting levels.
// 3. Formats exact Python separators (", ", ": ") and ensure_ascii=True escaping (including DEL and emojis).
func TestRegressionCanonicalFieldEvolution(t *testing.T) {
	// 1. Evolved schema with new fields at root and nested levels
	type EvolvedAddress struct {
		Building   string  `json:"building"`
		City       string  `json:"city"`
		Country    string  `json:"country"`
		Floor      int     `json:"floor"`
		PostalCode string  `json:"postal_code"`
		State      *string `json:"state"`
		Street     string  `json:"street"`
		ValidFrom  *string `json:"valid_from"`
		ValidTo    *string `json:"valid_to"`
	}

	type EvolvedProfile struct {
		Addresses      []EvolvedAddress  `json:"addresses"`
		BackupEmail    *string           `json:"backup_email"`
		DateOfBirth    *string           `json:"date_of_birth"`
		EmailAddresses []string          `json:"email_addresses"`
		FullName       string            `json:"full_name"`
		Jurisdictions  []string          `json:"jurisdictions"`
		MiddleName     string            `json:"middle_name"`
		NameVariants   []string          `json:"name_variants"`
		PhoneNumbers   []string          `json:"phone_numbers"`
		SecurityTier   int               `json:"security_tier"`
		TagMap         map[string]string `json:"tag_map"`
	}

	backup := "backup@example.com"
	dob := "1995-05-20"
	evolved := EvolvedProfile{
		FullName:       "Alex Evolved",
		MiddleName:     "Danger",
		BackupEmail:    &backup,
		DateOfBirth:    &dob,
		SecurityTier:   3,
		NameVariants:   []string{"Alex E."},
		EmailAddresses: []string{"alex@example.com"},
		PhoneNumbers:   []string{"+1-555-0100"},
		Jurisdictions:  []string{"US", "EU"},
		TagMap: map[string]string{
			"zebra": "last",
			"alpha": "first",
		},
		Addresses: []EvolvedAddress{
			{
				Building:   "Tower A",
				Floor:      12,
				City:       "Zurich",
				Country:    "CH",
				PostalCode: "8001",
				State:      nil,
				Street:     "Bahnhofstrasse 1",
				ValidFrom:  nil,
				ValidTo:    nil,
			},
		},
	}

	canonBytes, err := CanonicalGenericJSON(evolved)
	if err != nil {
		t.Fatalf("CanonicalGenericJSON failed on evolved struct: %v", err)
	}
	canon := string(canonBytes)

	// Verify all evolved fields are present
	for _, expectedKey := range []string{
		`"addresses": `, `"backup_email": "backup@example.com"`,
		`"date_of_birth": "1995-05-20"`, `"email_addresses": ["alex@example.com"]`,
		`"full_name": "Alex Evolved"`, `"jurisdictions": ["US", "EU"]`,
		`"middle_name": "Danger"`, `"name_variants": ["Alex E."]`,
		`"phone_numbers": ["+1-555-0100"]`, `"security_tier": 3`,
		`"tag_map": {"alpha": "first", "zebra": "last"}`,
		`"building": "Tower A"`, `"floor": 12`,
	} {
		if !strings.Contains(canon, expectedKey) {
			t.Errorf("canonical JSON missing expected evolved field %s:\n%s", expectedKey, canon)
		}
	}

	// Verify key order at root level:
	// addresses < backup_email < date_of_birth < email_addresses < full_name <
	// jurisdictions < middle_name < name_variants < phone_numbers < security_tier < tag_map
	expectedRootOrder := []string{
		`"addresses":`,
		`"backup_email":`,
		`"date_of_birth":`,
		`"email_addresses":`,
		`"full_name":`,
		`"jurisdictions":`,
		`"middle_name":`,
		`"name_variants":`,
		`"phone_numbers":`,
		`"security_tier":`,
		`"tag_map":`,
	}
	lastIdx := -1
	for _, key := range expectedRootOrder {
		idx := strings.Index(canon, key)
		if idx == -1 {
			t.Fatalf("key %s not found in canonical JSON", key)
		}
		if idx <= lastIdx {
			t.Fatalf("key %s appears at pos %d, expected after pos %d", key, idx, lastIdx)
		}
		lastIdx = idx
	}

	// 2. Unicode and character escaping verification:
	// Includes quotes, backslashes, tabs, DEL (\x7f), German umlauts, and Astral plane emoji (surrogate pairs)
	type EscapeVector struct {
		Text string `json:"text"`
	}
	vec := EscapeVector{
		Text: "Hello \"World\" \\ path\b\f\n\r\t \x1f \x7f München — 🚀",
	}
	escBytes, err := CanonicalGenericJSON(vec)
	if err != nil {
		t.Fatalf("CanonicalGenericJSON failed on escape vector: %v", err)
	}
	escStr := string(escBytes)

	// Check DEL (\x7f) escaped as \u007f
	if !strings.Contains(escStr, `\u007f`) {
		t.Errorf("expected DEL to be escaped as \\u007f, got: %s", escStr)
	}
	// Check control char \x1f escaped as \u001f
	if !strings.Contains(escStr, `\u001f`) {
		t.Errorf("expected \\x1f to be escaped as \\u001f, got: %s", escStr)
	}
	// Check München escaped as M\u00fcnchen
	if !strings.Contains(escStr, `M\u00fcnchen`) {
		t.Errorf("expected M\\u00fcnchen, got: %s", escStr)
	}
	// Check emoji 🚀 (U+1F680) escaped as surrogate pair \ud83d\ude80
	if !strings.Contains(escStr, `\ud83d\ude80`) {
		t.Errorf("expected emoji surrogate pair \\ud83d\\ude80, got: %s", escStr)
	}
}
