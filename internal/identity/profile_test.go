package identity

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func testKey(t *testing.T) []byte {
	t.Helper()
	k := []byte("0123456789abcdef0123456789abcdef")
	if err := SetMasterKey(k); err != nil {
		t.Fatal(err)
	}
	return k
}

func clearKey(t *testing.T) {
	t.Helper()
	// Identity tests share process-global key, provider, and keyring state.
	// Reset all of it before each test so ambient credentials and preceding
	// tests cannot change which key source GetExistingMasterKey observes.
	Shutdown()
	SetKeyringBackend(NewFakeKeyring())
	t.Setenv(EnvMasterKeyHex, "")
	t.Setenv(EnvSymvaultPassphrase, "")
	t.Cleanup(func() {
		Shutdown()
		SetKeyringBackend(nil)
	})
}

// TestProfileRoundTrip: Save→Load preserves the profile (encrypted at rest).
func TestProfileRoundTrip(t *testing.T) {
	clearKey(t)
	_ = testKey(t)
	dir := t.TempDir()
	path := filepath.Join(dir, "identity.encrypted")

	p := &Profile{
		FullName:       "Test Person",
		NameVariants:   []string{"T. Person"},
		EmailAddresses: []string{"person@example.com"},
		Jurisdictions:  []string{"DE", "EU"},
		Addresses: []Address{{
			Street: "Musterweg 1", City: "Berlin", PostalCode: "10115", Country: "DE",
		}},
	}
	if _, err := SaveProfile(p, path); err != nil {
		t.Fatalf("save: %v", err)
	}
	got, err := LoadProfile(path)
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if got.FullName != p.FullName || len(got.EmailAddresses) != 1 || got.Addresses[0].City != "Berlin" {
		t.Errorf("round-trip mismatch: %+v", got)
	}

	// The file on disk must NOT contain the plaintext.
	raw, _ := os.ReadFile(path)
	if strings.Contains(string(raw), "Test Person") {
		t.Fatal("profile plaintext leaked to disk")
	}
	if !strings.Contains(string(raw), "AES-256-GCM") {
		t.Error("expected AES-256-GCM envelope header")
	}
}

// TestLoadProfileMissing yields the dedicated error.
func TestLoadProfileMissing(t *testing.T) {
	clearKey(t)
	_, err := LoadProfile(filepath.Join(t.TempDir(), "nope.encrypted"))
	if err != ErrProfileNotFound {
		t.Errorf("want ErrProfileNotFound, got %v", err)
	}
}

// TestWrongKeyFails: decrypting with a different master key fails cleanly.
func TestWrongKeyFails(t *testing.T) {
	clearKey(t)
	_ = SetMasterKey([]byte("key-A-0123456789abcdef0123456789"))
	dir := t.TempDir()
	path := filepath.Join(dir, "identity.encrypted")
	if _, err := SaveProfile(&Profile{FullName: "A"}, path); err != nil {
		t.Fatal(err)
	}
	_ = SetMasterKey([]byte("key-B-0123456789abcdef0123456789"))
	if _, err := LoadProfile(path); err == nil {
		t.Fatal("expected failure with wrong key")
	} else if !strings.Contains(err.Error(), "corrupt") {
		t.Errorf("unexpected error type: %v", err)
	}
}

// TestHashProfileDeterministic.
func TestHashProfileDeterministic(t *testing.T) {
	p := &Profile{FullName: "X", EmailAddresses: []string{"a@b.example.com"}}
	first := HashProfile(p)
	if first != HashProfile(p) {
		t.Error("hash not deterministic")
	}
}

// TestLegacyV0Rejected.
func TestLegacyV0Rejected(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "legacy.encrypted")
	header := `{"version":0,"nonce":"00","algorithm":"AES-256-GCM"}`
	if err := os.WriteFile(path, []byte(header+"\n\x00\x01"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := LoadProfile(path); err != ErrLegacyV0Unsupported {
		t.Errorf("want ErrLegacyV0Unsupported, got %v", err)
	}
}

// TestMasterKeyEnvPrecedence: direct hex beats the symvault passphrase.
func TestMasterKeyEnvPrecedence(t *testing.T) {
	clearKey(t)
	hexKey := "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899"
	t.Setenv(EnvMasterKeyHex, hexKey)
	t.Setenv(EnvSymvaultPassphrase, "should-be-ignored")
	k, err := GetExistingMasterKey()
	if err != nil {
		t.Fatal(err)
	}
	if len(k) != KeyLength {
		t.Fatalf("key length %d", len(k))
	}
	// The hex value must be used verbatim (not hashed).
	want, _ := hexDecode(hexKey)
	for i := range want {
		if k[i] != want[i] {
			t.Fatal("hex key not used verbatim")
		}
	}
}

// TestPassphraseDerivationDeterministic: the same passphrase yields the
// same key; different passphrases differ.
func TestPassphraseDerivationDeterministic(t *testing.T) {
	clearKey(t)
	t.Setenv(EnvSymvaultPassphrase, "op-1")
	k1, err := GetExistingMasterKey()
	if err != nil {
		t.Fatal(err)
	}
	_ = SetMasterKey(nil)
	t.Setenv(EnvSymvaultPassphrase, "op-1")
	k2, _ := GetExistingMasterKey()
	if string(k1) != string(k2) {
		t.Error("passphrase derivation not deterministic")
	}
	_ = SetMasterKey(nil)
	t.Setenv(EnvSymvaultPassphrase, "op-2")
	k3, _ := GetExistingMasterKey()
	if string(k1) == string(k3) {
		t.Error("different passphrases must yield different keys")
	}
}

func hexDecode(s string) ([]byte, error) {
	out := make([]byte, len(s)/2)
	const hexDigits = "0123456789abcdef"
	idx := func(c byte) int {
		for i := 0; i < len(hexDigits); i++ {
			if hexDigits[i] == c || hexDigits[i] == c|0x20 {
				return i
			}
		}
		return -1
	}
	for i := 0; i < len(out); i++ {
		hi := idx(s[2*i])
		lo := idx(s[2*i+1])
		if hi < 0 || lo < 0 {
			return nil, os.ErrInvalid
		}
		out[i] = byte(hi<<4 | lo)
	}
	return out, nil
}
