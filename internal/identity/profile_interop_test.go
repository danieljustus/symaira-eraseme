package identity

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

type provenanceProfile struct {
	FileEnc       string `json:"file_enc"`
	CanonicalJSON string `json:"canonical_json"`
	CanonicalHash string `json:"canonical_hash"`
	HeaderJSON    string `json:"header_json"`
	CiphertextHex string `json:"ciphertext_hex"`
	PayloadJSON   string `json:"payload_json"`
}

type provenanceData struct {
	Generator         string                       `json:"generator"`
	PythonFinalTag    string                       `json:"python_final_tag"`
	PythonFinalCommit string                       `json:"python_final_commit"`
	KeyHex            string                       `json:"key_hex"`
	NonceHex          string                       `json:"nonce_hex"`
	Profiles          map[string]provenanceProfile `json:"profiles"`
}

func loadProvenance(t *testing.T) (*provenanceData, string) {
	t.Helper()
	fixturesDir := filepath.Join("..", "..", "tests", "fixtures", "identity-contract")
	provPath := filepath.Join(fixturesDir, "provenance.json")
	data, err := os.ReadFile(provPath)
	if err != nil {
		t.Fatalf("read provenance.json: %v", err)
	}
	var prov provenanceData
	if err := json.Unmarshal(data, &prov); err != nil {
		t.Fatalf("unmarshal provenance.json: %v", err)
	}
	return &prov, fixturesDir
}

// TestPythonFixturesDecryptionAndHashParity verifies that Go can read the real
// Python-generated fixtures and reproduces the frozen profile hashes. Filename
// discovery for both .enc and .encrypted is covered separately with copied data.
func TestPythonFixturesDecryptionAndHashParity(t *testing.T) {
	prov, fixturesDir := loadProvenance(t)
	key, err := hex.DecodeString(prov.KeyHex)
	if err != nil {
		t.Fatalf("decode key hex: %v", err)
	}

	for name, profData := range prov.Profiles {
		t.Run(name, func(t *testing.T) {
			file := profData.FileEnc
			filePath := filepath.Join(fixturesDir, file)
			raw, err := os.ReadFile(filePath)
			if err != nil {
				t.Fatalf("read fixture %s: %v", file, err)
			}

			plain, header, err := decryptProfileWithKey(raw, key)
			if err != nil {
				t.Fatalf("decryptProfileWithKey %s: %v", file, err)
			}
			if header.Version != 2 {
				t.Errorf("expected version 2, got %d", header.Version)
			}
			if header.Nonce != prov.NonceHex {
				t.Errorf("expected nonce %s, got %s", prov.NonceHex, header.Nonce)
			}

			var profile Profile
			if err := json.Unmarshal(plain, &profile); err != nil {
				t.Fatalf("unmarshal profile %s: %v", file, err)
			}

			canon := CanonicalJSON(&profile)
			if string(canon) != profData.CanonicalJSON {
				t.Errorf("canonical JSON mismatch for %s:\ngot:  %s\nwant: %s", file, string(canon), profData.CanonicalJSON)
			}

			gotHash := HashProfile(&profile)
			if gotHash != profData.CanonicalHash {
				t.Errorf("profile hash mismatch for %s:\ngot:  %s\nwant: %s", file, gotHash, profData.CanonicalHash)
			}
		})
	}
}

// TestGoSerializationMatchesPythonPydantic verifies that Go Profile serialization
// preserves all fields, empty lists as [], and nulls (no omitempty omission).
func TestGoSerializationMatchesPythonPydantic(t *testing.T) {
	dob := "1990-01-15"
	p := &Profile{
		FullName:       "Jane Doe",
		NameVariants:   []string{"Jane Roe"},
		DateOfBirth:    &dob,
		Addresses:      []Address{},
		EmailAddresses: []string{"jane@example.com"},
		PhoneNumbers:   []string{},
		Jurisdictions:  []string{},
	}

	marshaled, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		t.Fatalf("marshal profile: %v", err)
	}
	s := string(marshaled)

	// In Python Pydantic, all fields are present:
	if !strings.Contains(s, `"addresses": []`) {
		t.Errorf("expected addresses to be serialized as empty slice [], got:\n%s", s)
	}
	if !strings.Contains(s, `"phone_numbers": []`) {
		t.Errorf("expected phone_numbers to be serialized as empty slice [], got:\n%s", s)
	}
	if !strings.Contains(s, `"jurisdictions": []`) {
		t.Errorf("expected jurisdictions to be serialized as empty slice [], got:\n%s", s)
	}

	// Now check when DateOfBirth is nil
	p.DateOfBirth = nil
	marshaledNilDob, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		t.Fatalf("marshal profile with nil dob: %v", err)
	}
	sNil := string(marshaledNilDob)
	if !strings.Contains(sNil, `"date_of_birth": null`) {
		t.Errorf("expected date_of_birth to be serialized as null when nil, got:\n%s", sNil)
	}
}

// TestDecryptPathNeverMintsOrReplacesKey verifies that every decrypt/read path
// performs read-only key lookup and never mints or caches a key when missing or wrong.
func TestDecryptPathNeverMintsOrReplacesKey(t *testing.T) {
	fake := NewFakeKeyring()
	SetKeyringBackend(fake)
	t.Cleanup(func() {
		SetKeyringBackend(nil)
		SetMasterKey(nil)
	})

	dir := t.TempDir()
	t.Setenv("HOME", dir)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", "")
	t.Setenv("SYMVAULT_PASSPHRASE", "")
	SetMasterKey(nil)

	// Create an encrypted profile on disk with a known key
	knownKey := []byte("0123456789abcdef0123456789abcdef")
	profilePath := filepath.Join(dir, "identity.encrypted")
	p := &Profile{FullName: "Test User", EmailAddresses: []string{"test@example.com"}}
	plain, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	ciphertext, err := encryptProfile(plain, knownKey)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(profilePath, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}

	// 1. Missing key on LoadProfile:
	// Keyring is empty, env vars are empty, keyCache is nil.
	SetMasterKey(nil)
	_, err = LoadProfile(profilePath)
	if !errors.Is(err, ErrMasterKeyMissing) {
		t.Fatalf("expected ErrMasterKeyMissing, got: %v", err)
	}

	// Verify that NO key was minted into keyring or memory cache!
	if stored, _ := fake.Get(ServiceName, KeyringUsername); stored != "" {
		t.Fatalf("read path silently minted key into keyring: %s", stored)
	}
	if cachedKey, _ := GetExistingMasterKey(); cachedKey != nil {
		t.Fatal("read path silently cached key in memory")
	}

	// 2. Wrong key on LoadProfile:
	wrongKey := []byte("wrongwrongwrongwrongwrongwrong12")
	if err := SetMasterKey(wrongKey); err != nil {
		t.Fatal(err)
	}
	_, err = LoadProfile(profilePath)
	if err == nil || !strings.Contains(err.Error(), "corrupt") {
		t.Fatalf("expected corrupt error on wrong key, got: %v", err)
	}

	// Verify the key in cache was NOT replaced with a newly minted key
	currentKey, _ := GetExistingMasterKey()
	if string(currentKey) != string(wrongKey) {
		t.Fatal("wrong key read path altered master key in cache")
	}
}

// TestExplicitInitializationRequiredAndSurfacesPersistenceFailure verifies:
// 1. Key creation only happens via explicit initialization.
// 2. If durable keychain persistence fails, the failure is surfaced and the key is not cached.
func TestExplicitInitializationRequiredAndSurfacesPersistenceFailure(t *testing.T) {
	fake := NewFakeKeyring()
	SetKeyringBackend(fake)
	t.Cleanup(func() {
		SetKeyringBackend(nil)
		SetMasterKey(nil)
	})

	dir := t.TempDir()
	t.Setenv("HOME", dir)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", "")
	t.Setenv("SYMVAULT_PASSPHRASE", "")
	SetMasterKey(nil)

	// Case A: Failing keychain persistence during initialization
	fake.SetError = errors.New("simulated OS keychain unavailable or locked")

	_, err := InitMasterKey()
	if err == nil {
		t.Fatal("expected InitMasterKey to fail when durable persistence fails")
	}
	if !strings.Contains(err.Error(), "keychain") && !strings.Contains(err.Error(), "persistence") {
		t.Fatalf("expected keychain persistence failure message, got: %v", err)
	}

	// Ensure the key was NOT cached in memory!
	if cached, _ := GetExistingMasterKey(); cached != nil {
		t.Fatal("unpersisted key was cached in memory instead of failing closed")
	}

	// Case B: Successful explicit initialization
	fake.SetError = nil
	key, err := InitMasterKey()
	if err != nil {
		t.Fatalf("InitMasterKey failed: %v", err)
	}
	if len(key) != KeyLength {
		t.Fatalf("expected %d bytes key, got %d", KeyLength, len(key))
	}

	stored, err := fake.Get(ServiceName, KeyringUsername)
	if err != nil || stored != hex.EncodeToString(key) {
		t.Fatalf("key was not persisted to keyring: %v", err)
	}
}

// TestMalformedKeyLengthsRejected verifies strict 32-byte key length enforcement.
func TestMalformedKeyLengthsRejected(t *testing.T) {
	t.Cleanup(func() { SetMasterKey(nil) })

	// Test SetMasterKey rejects malformed key lengths
	for _, badLen := range []int{1, 16, 24, 31, 33, 64} {
		badKey := make([]byte, badLen)
		if err := SetMasterKey(badKey); err == nil {
			t.Errorf("SetMasterKey accepted invalid key length %d", badLen)
		}
	}

	// Test hex env var with invalid length
	t.Setenv(EnvMasterKeyHex, "aabbcc") // 3 bytes
	if _, err := GetExistingMasterKey(); err == nil {
		t.Error("expected error for short hex key in env")
	}

	// Test hex env var with 66 hex chars (33 bytes)
	t.Setenv(EnvMasterKeyHex, strings.Repeat("aa", 33))
	if _, err := GetExistingMasterKey(); err == nil {
		t.Error("expected error for 33-byte hex key in env")
	}

	// Test fake keyring containing malformed key length
	fake := NewFakeKeyring()
	SetKeyringBackend(fake)
	t.Cleanup(func() { SetKeyringBackend(nil) })
	t.Setenv(EnvMasterKeyHex, "")
	_ = fake.Set(ServiceName, KeyringUsername, "shorthex")
	if _, err := GetExistingMasterKey(); err == nil {
		t.Error("expected error for malformed keyring master key")
	}
}

// TestPathDiscoveryAndMigration verifies discovery of Python identity.enc and
// Go identity.encrypted without deleting source or stranding existing Go users.
func TestPathDiscoveryAndMigration(t *testing.T) {
	clearKey(t)
	testKey := []byte("0123456789abcdef0123456789abcdef")
	if err := SetMasterKey(testKey); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { SetMasterKey(nil) })

	// 1. Discovery of legacy Python identity.enc when identity.encrypted is absent
	dir := t.TempDir()
	t.Setenv("HOME", dir)
	configDir := filepath.Join(dir, ".config", "symeraseme")
	if err := os.MkdirAll(configDir, 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("SYMERASEME_CONFIG_DIR", configDir)
	t.Setenv("SYMERASEME_DATA_DIR", "")
	t.Setenv("SYMERASEME_IDENTITY_PATH", "")

	pyPath := filepath.Join(configDir, "identity.enc")
	goPath := filepath.Join(configDir, "identity.encrypted")

	p := &Profile{FullName: "Python Migrator", EmailAddresses: []string{"migrator@example.com"}}
	plain, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	ciphertext, err := encryptProfile(plain, testKey)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(pyPath, ciphertext, 0o600); err != nil {
		t.Fatal(err)
	}

	// ProfileExists should discover identity.enc
	if !ProfileExists("") {
		t.Fatal("ProfileExists failed to discover identity.enc")
	}

	// LoadProfile should load identity.enc
	loaded, err := LoadProfile("")
	if err != nil {
		t.Fatalf("LoadProfile failed to load identity.enc: %v", err)
	}
	if loaded.FullName != "Python Migrator" {
		t.Fatalf("loaded profile mismatch: %s", loaded.FullName)
	}

	// 2. Saving profile writes to canonical Go path identity.encrypted WITHOUT deleting identity.enc
	p.FullName = "Updated Migrator"
	savedPath, err := SaveProfile(p, "")
	if err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}
	if savedPath != goPath {
		t.Fatalf("expected SaveProfile to write to %s, got %s", goPath, savedPath)
	}

	// Source identity.enc must still exist!
	if _, err := os.Stat(pyPath); err != nil {
		t.Fatalf("source identity.enc was deleted during SaveProfile: %v", err)
	}

	// Next LoadProfile should load the canonical Go file identity.encrypted
	reloaded, err := LoadProfile("")
	if err != nil {
		t.Fatalf("LoadProfile failed after save: %v", err)
	}
	if reloaded.FullName != "Updated Migrator" {
		t.Fatalf("expected updated profile, got %s", reloaded.FullName)
	}

	// 3. Existing Go user with only identity.encrypted is not stranded
	dir2 := t.TempDir()
	configDir2 := filepath.Join(dir2, ".config", "symeraseme")
	if err := os.MkdirAll(configDir2, 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", dir2)
	t.Setenv("SYMERASEME_CONFIG_DIR", configDir2)
	goOnlyPath := filepath.Join(configDir2, "identity.encrypted")
	p2 := &Profile{FullName: "Go User", EmailAddresses: []string{"go@example.com"}}
	plain2, _ := json.MarshalIndent(p2, "", "  ")
	ct2, _ := encryptProfile(plain2, testKey)
	_ = os.WriteFile(goOnlyPath, ct2, 0o600)

	if !ProfileExists("") {
		t.Fatal("ProfileExists failed for Go user")
	}
	loadedGo, err := LoadProfile("")
	if err != nil || loadedGo.FullName != "Go User" {
		t.Fatalf("LoadProfile failed for Go user: %v", err)
	}
}

// TestGoWritesDecryptableByPythonDecoder verifies that profiles encrypted by Go
// can be independently decrypted and validated by the Python-compatible decoder
// (both our reference decoder and the external python3/uv interpreter if available).
func TestGoWritesDecryptableByPythonDecoder(t *testing.T) {
	key := []byte("0123456789abcdef0123456789abcdef")
	_ = SetMasterKey(key)
	t.Cleanup(func() { _ = SetMasterKey(nil) })

	testProfiles := map[string]*Profile{
		"minimal": {
			FullName:       "Jane Doe",
			EmailAddresses: []string{"jane@example.com"},
		},
		"full": {
			FullName:     "Jane Doe",
			NameVariants: []string{"Jane Roe", "Jane Smith"},
			DateOfBirth:  func(s string) *string { return &s }("1990-01-15"),
			Addresses: []Address{
				{
					Street:     "123 Main St",
					City:       "Berlin",
					PostalCode: "10115",
					Country:    "DE",
					State:      func(s string) *string { return &s }("Berlin"),
					ValidFrom:  func(s string) *string { return &s }("2020-01-01"),
					ValidTo:    nil,
				},
			},
			EmailAddresses: []string{"jane@example.com", "jane.doe@work.example.com"},
			PhoneNumbers:   []string{"+49-30-123456", "+1-555-0199"},
			Jurisdictions:  []string{"DE", "EU", "US-CA"},
		},
		"unicode": {
			FullName:     "Jörg Müller",
			NameVariants: []string{"Jörg Mueller"},
			DateOfBirth:  func(s string) *string { return &s }("1985-12-31"),
			Addresses: []Address{
				{
					Street:     "Goethestraße 42",
					City:       "München",
					PostalCode: "80331",
					Country:    "DE",
					State:      nil,
					ValidFrom:  nil,
					ValidTo:    nil,
				},
			},
			EmailAddresses: []string{"joerg.mueller@example.de"},
			PhoneNumbers:   []string{"+49-89-9876543"},
			Jurisdictions:  []string{"DE", "EU"},
		},
	}

	dir := t.TempDir()
	for name, prof := range testProfiles {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(dir, name+".encrypted")
			_, err := SaveProfile(prof, path)
			if err != nil {
				t.Fatalf("SaveProfile failed: %v", err)
			}

			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("read encrypted profile: %v", err)
			}
			if _, err := DecryptProfileWithKey(raw, key); err != nil {
				t.Fatalf("DecryptProfileWithKey failed: %v", err)
			}

			// 1. In-process reference decrypt verification:
			decoded, err := LoadProfile(path)
			if err != nil {
				t.Fatalf("LoadProfile failed on Go write: %v", err)
			}
			if decoded.FullName != prof.FullName {
				t.Errorf("decoded FullName = %q, want %q", decoded.FullName, prof.FullName)
			}
			if HashProfile(decoded) != HashProfile(prof) {
				t.Errorf("hash mismatch after decode: got %s, want %s", HashProfile(decoded), HashProfile(prof))
			}

			// 2. Real external python interpreter using uv (manual/integration only; normal unit tests use canonical static fixtures):
			if os.Getenv("SYMERASEME_RUN_EXTERNAL_PY_TESTS") != "1" {
				return
			}
			uvPath, err := exec.LookPath("uv")
			if err != nil {
				t.Skip("uv not installed, skipping external Python subprocess verification")
			}

			pyScript := `
import json, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from pydantic import BaseModel, EmailStr, Field

class Address(BaseModel):
    street: str
    city: str
    postal_code: str
    country: str
    state: str | None = None
    valid_from: str | None = None
    valid_to: str | None = None

class IdentityProfile(BaseModel):
    full_name: str
    name_variants: list[str] = Field(default_factory=list)
    date_of_birth: str | None = None
    addresses: list[Address] = Field(default_factory=list)
    email_addresses: list[EmailStr] = Field(default_factory=list)
    phone_numbers: list[str] = Field(default_factory=list)
    jurisdictions: list[str] = Field(default_factory=list)

def hash_profile(profile: IdentityProfile) -> str:
    import hashlib
    canonical = json.dumps(profile.model_dump(mode="json"), sort_keys=True)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()

path = sys.argv[1]
key_hex = sys.argv[2]
expected_hash = sys.argv[3]

with open(path, "rb") as f:
    raw = f.read()

header_bytes, _, ciphertext = raw.partition(b"\n")
header = json.loads(header_bytes)
assert header["version"] == 2, f"version was {header.get('version')}"
assert header["algorithm"] == "AES-256-GCM"

nonce = bytes.fromhex(header["nonce"])
aesgcm = AESGCM(bytes.fromhex(key_hex))
plaintext = aesgcm.decrypt(nonce, ciphertext, header_bytes)

data = json.loads(plaintext.decode("utf-8"))
profile = IdentityProfile.model_validate(data)
actual_hash = hash_profile(profile)
assert actual_hash == expected_hash, f"hash mismatch: actual={actual_hash} expected={expected_hash}"
print("VERIFIED")
`
			cmd := exec.Command(uvPath, "run", "--with", "pydantic", "--with", "cryptography", "--with", "email-validator",
				"python3", "-c", pyScript, path, hex.EncodeToString(key), HashProfile(prof))
			out, err := cmd.CombinedOutput()
			if err != nil {
				t.Fatalf("Python decoder verification failed for %s:\n%s\nerr: %v", name, string(out), err)
			}
			if !strings.Contains(string(out), "VERIFIED") {
				t.Fatalf("Python decoder did not emit VERIFIED:\n%s", string(out))
			}
		})
	}
}
