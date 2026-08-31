package identity

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"golang.org/x/crypto/scrypt"
)

func TestMasterKeyCacheAndProfileCipherHelpers(t *testing.T) {
	SetMasterKey(nil)
	t.Cleanup(func() { SetMasterKey(nil) })
	SetMasterKey([]byte("short key is hashed"))
	first, err := GetExistingMasterKey()
	if err != nil || len(first) != KeyLength {
		t.Fatalf("hashed cached key = %d, err=%v", len(first), err)
	}
	first[0] ^= 1
	second, err := GetExistingMasterKey()
	if err != nil || first[0] == second[0] {
		t.Fatal("cached key was returned by reference")
	}
	key := []byte("12345678901234567890123456789012")
	plain := []byte(`{"full_name":"Test Person"}`)
	ciphertext, err := EncryptProfileWithKey(plain, key)
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := DecryptProfileWithKey(ciphertext, key)
	if err != nil || string(decoded) != string(plain) {
		t.Fatalf("cipher helper round trip = %q, err=%v", decoded, err)
	}
	if _, err := DecryptProfileWithKey(ciphertext, []byte("wrong-key")); err == nil {
		t.Fatal("wrong profile key accepted")
	}
	for _, raw := range [][]byte{[]byte("no separator"), []byte("not-json\nbody")} {
		if _, err := DecryptProfileWithKey(raw, key); err == nil {
			t.Fatalf("invalid profile envelope accepted: %q", raw)
		}
	}
}

func TestMasterKeyEnvironmentValidationAndGeneration(t *testing.T) {
	SetMasterKey(nil)
	t.Cleanup(func() { SetMasterKey(nil) })
	t.Setenv(EnvMasterKeyHex, "not-hex")
	if _, err := GetExistingMasterKey(); err == nil {
		t.Fatal("invalid hex master key accepted")
	}
	t.Setenv(EnvMasterKeyHex, "aa")
	if _, err := GetExistingMasterKey(); err == nil {
		t.Fatal("short hex master key accepted")
	}
	t.Setenv(EnvMasterKeyHex, "")
	t.Setenv(EnvSymvaultPassphrase, "passphrase")
	key, err := GetExistingMasterKey()
	if err != nil || len(key) != KeyLength {
		t.Fatalf("passphrase key = %d, err=%v", len(key), err)
	}
	generated, err := GenerateMasterKey()
	if err != nil || len(generated) != KeyLength {
		t.Fatalf("generated key = %d, err=%v", len(generated), err)
	}
	if string(generated) == string(key) {
		t.Fatal("generated key unexpectedly equals passphrase key")
	}
}

func TestPassphraseUsesMemoryHardDerivation(t *testing.T) {
	SetMasterKey(nil)
	t.Cleanup(func() { SetMasterKey(nil) })
	t.Setenv(EnvMasterKeyHex, "")
	t.Setenv(EnvSymvaultPassphrase, "passphrase")

	key, err := GetExistingMasterKey()
	if err != nil {
		t.Fatal(err)
	}
	want, err := scrypt.Key([]byte("passphrase"), identityPassphraseSalt, 1<<15, 8, 1, KeyLength)
	if err != nil {
		t.Fatal(err)
	}
	if string(key) != string(want) {
		t.Fatal("passphrase did not use the configured scrypt derivation")
	}
}

func TestConsentGatePrecedenceAndFileParsing(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dir)
	SetNowFunc(func() int64 { return 1000 })
	t.Cleanup(func() { SetNowFunc(nil) })
	if err := ConsentGate("delete", ConsentOptions{Yes: true}); err != nil {
		t.Fatal(err)
	}
	if err := ConsentGate("delete", ConsentOptions{}); !errors.Is(err, ErrConsentDenied) {
		t.Fatalf("missing consent error = %v", err)
	}

	token, err := IssueToken("delete", 60)
	if err != nil {
		t.Fatal(err)
	}
	if err := ConsentGate("delete", ConsentOptions{ConsentToken: token}); err != nil {
		t.Fatal(err)
	}
	if err := VerifyToken("delete", token); err == nil {
		t.Fatal("explicit consent token was not consumed")
	}

	fileToken, err := IssueToken("delete", 60)
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(dir, "consent.txt")
	if err := os.WriteFile(file, []byte(fileToken+"\nignored"), 0o644); err != nil {
		t.Fatal(err)
	}
	value, err := ReadConsentFile(file)
	if err != nil || value != fileToken {
		t.Fatalf("ReadConsentFile = %q, err=%v", value, err)
	}
	if err := ConsentGate("delete", ConsentOptions{ConsentFile: file}); err != nil {
		t.Fatal(err)
	}

	envToken, err := IssueToken("delete", 60)
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("TEST_CONSENT_TOKEN", envToken)
	if err := ConsentGate("delete", ConsentOptions{ConsentEnvVar: "TEST_CONSENT_TOKEN"}); err != nil {
		t.Fatal(err)
	}

	if value, err := ReadConsentFile(""); err != nil || value != "" {
		t.Fatalf("empty consent path = %q, err=%v", value, err)
	}
	if _, err := ReadConsentFile("   "); err == nil {
		t.Fatal("whitespace path unexpectedly succeeded")
	}
	if _, err := ReadConsentFile(filepath.Join(dir, "missing")); err == nil {
		t.Fatal("missing consent file unexpectedly succeeded")
	}
	if _, err := ParseExpiry("not-an-int"); err == nil {
		t.Fatal("invalid expiry accepted")
	}
}

func TestConsentListPrunesExpiredAndIgnoresMalformedFiles(t *testing.T) {
	dir := t.TempDir()
	SetNowFunc(func() int64 { return 2000 })
	t.Cleanup(func() { SetNowFunc(nil) })
	fresh, err := IssueTokenInDir(dir, "fresh", 100)
	if err != nil {
		t.Fatal(err)
	}
	_, err = IssueTokenInDir(dir, "expired", 1)
	if err != nil {
		t.Fatal(err)
	}
	SetNowFunc(func() int64 { return 2002 })
	if err := os.WriteFile(filepath.Join(dir, "consent_bad.json"), []byte("{"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "not-consent.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	tokens, err := ListTokensInDir(dir)
	if err != nil || len(tokens) != 1 || tokens[0].Token != fresh {
		t.Fatalf("tokens = %#v, err=%v", tokens, err)
	}
	if _, err := os.Stat(filepath.Join(dir, "consent_bad.json")); err != nil {
		t.Fatal("malformed token was removed instead of ignored")
	}
}

func TestBootstrapReadOnlyAndShutdown(t *testing.T) {
	SetMasterKey([]byte("bootstrap-test-key"))
	t.Cleanup(Shutdown)
	key, err := Bootstrap()
	if err != nil || len(key) != KeyLength {
		t.Fatalf("Bootstrap key = %d, err=%v", len(key), err)
	}
	if err := BootstrapReadOnly(); err != nil {
		t.Fatal(err)
	}
	Shutdown()
	t.Setenv(EnvMasterKeyHex, "not-a-valid-key")
	if _, err := GetExistingMasterKey(); err == nil {
		t.Fatal("Shutdown did not clear master key cache")
	}
	// Reset the eventstore provider explicitly so this test cannot leak state.
	eventstore.SetMasterKeyProvider(nil)
}

func TestRandomURLSafeAndProfilePathPrecedence(t *testing.T) {
	if token := RandomURLSafe(0); len(token) == 0 {
		t.Fatal("default random token is empty")
	}
	if token := RandomURLSafe(32); len(token) < 40 || strings.ContainsAny(token, "/+") {
		t.Fatalf("token is not URL-safe: %q", token)
	}
	t.Setenv("SYMERASEME_DATA_DIR", filepath.Join(os.TempDir(), "identity-test-data"))
	path, err := DefaultProfilePath()
	if err != nil || !strings.HasSuffix(path, filepath.Join("identity-test-data", "identity.encrypted")) {
		t.Fatalf("data-dir profile path = %q, err=%v", path, err)
	}
	t.Setenv("SYMERASEME_IDENTITY_PATH", "~/custom-profile")
	path, err = DefaultProfilePath()
	if err != nil || !strings.HasSuffix(path, filepath.Join("custom-profile")) {
		t.Fatalf("explicit profile path = %q, err=%v", path, err)
	}
}
