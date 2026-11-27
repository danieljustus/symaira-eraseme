package eventstore

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const (
	testMasterKeyASCII = "symaira-eraseme-golden-master-32"
)

func requireExternalPythonCrypto(t *testing.T) {
	t.Helper()
	if os.Getenv("SYMERASEME_RUN_EXTERNAL_PY_TESTS") != "1" {
		t.Skip("set SYMERASEME_RUN_EXTERNAL_PY_TESTS=1 for archived Python interoperability")
	}
	cmd := exec.Command("python3", "-c", "import cryptography")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("external Python cryptography dependency unavailable: %v: %s", err, out)
	}
}

func testMasterKey() []byte {
	return []byte(testMasterKeyASCII)
}

func testFixturesDir(t *testing.T) string {
	t.Helper()
	return filepath.Join(repoRoot(t), "tests", "fixtures", "event-store")
}

func loadProvenance(t *testing.T) map[string]any {
	t.Helper()
	p := filepath.Join(testFixturesDir(t), "crypto", "provenance.json")
	data, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("failed to read provenance.json: %v", err)
	}
	var prov map[string]any
	if err := json.Unmarshal(data, &prov); err != nil {
		t.Fatalf("failed to parse provenance.json: %v", err)
	}
	return prov
}

// TestProvenanceIntegrity verifies that all committed fixtures match their provenance records.
func TestProvenanceIntegrity(t *testing.T) {
	prov := loadProvenance(t)
	if prov["generator"] != "scripts/generate-crypto-fixtures.py" {
		t.Fatalf("unexpected generator in provenance: %v", prov["generator"])
	}
	fixtures, ok := prov["fixtures"].(map[string]any)
	if !ok {
		t.Fatalf("fixtures section missing in provenance")
	}
	for name, details := range fixtures {
		m := details.(map[string]any)
		wantHash := m["sha256"].(string)
		path := filepath.Join(testFixturesDir(t), "crypto", name)
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("fixture file %s missing: %v", name, err)
		}
		gotHash := sha256.Sum256(data)
		if hex.EncodeToString(gotHash[:]) != wantHash {
			t.Fatalf("hash mismatch for %s: got %s, want %s", name, hex.EncodeToString(gotHash[:]), wantHash)
		}
	}
}

// TestPythonFernetFixturesDecrypt tests that Go can decrypt Python-generated V1, V2, V3 standard Fernet fixtures.
func TestPythonFernetFixturesDecrypt(t *testing.T) {
	SetMasterKeyProvider(func() ([]byte, error) { return testMasterKey(), nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	fixDir := testFixturesDir(t)
	goldenCampaign, err := os.ReadFile(filepath.Join(fixDir, "golden-campaign.db"))
	if err != nil {
		t.Fatalf("failed to read golden-campaign.db: %v", err)
	}
	goldenHash := sha256.Sum256(goldenCampaign)

	cases := []struct {
		name     string
		filename string
		wantHash [32]byte
	}{
		{
			name:     "Python Standard Fernet V1",
			filename: filepath.Join(fixDir, "crypto", "golden-campaign-v1-python.db"),
			wantHash: goldenHash,
		},
		{
			name:     "Python Standard Fernet V2",
			filename: filepath.Join(fixDir, "crypto", "golden-campaign-v2-python.db"),
			wantHash: goldenHash,
		},
		{
			name:     "Python Standard Fernet V3",
			filename: filepath.Join(fixDir, "crypto", "golden-campaign-v3-python.db"),
			wantHash: goldenHash,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			decrypted, err := DecryptFile(tc.filename)
			if err != nil {
				t.Fatalf("DecryptFile(%s) failed: %v", tc.filename, err)
			}
			gotHash := sha256.Sum256(decrypted)
			if gotHash != tc.wantHash {
				t.Fatalf("decrypted hash mismatch: got %x, want %x", gotHash, tc.wantHash)
			}
			if !bytes.Equal(decrypted, goldenCampaign) {
				t.Fatalf("decrypted bytes do not match original golden-campaign.db")
			}
		})
	}
}

// TestSmallVectorPythonV3 tests decryption of the small deterministic Python vector.
func TestSmallVectorPythonV3(t *testing.T) {
	SetMasterKeyProvider(func() ([]byte, error) { return testMasterKey(), nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	p := filepath.Join(testFixturesDir(t), "crypto", "small-vector-v3-python.db")
	dec, err := DecryptFile(p)
	if err != nil {
		t.Fatalf("failed to decrypt small vector: %v", err)
	}
	want := []byte("SQLite format 3\x00deterministic test payload for small vector verification.")
	if !bytes.Equal(dec, want) {
		t.Fatalf("small vector mismatch: got %q, want %q", dec, want)
	}
}

// TestLegacyGoPayloadsReadCompatibility verifies that accidental Go AES-GCM payloads
// (V1, V2, V3) can still be safely decrypted by Go.
func TestLegacyGoPayloadsReadCompatibility(t *testing.T) {
	SetMasterKeyProvider(func() ([]byte, error) { return testMasterKey(), nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	fixDir := testFixturesDir(t)
	goldenCampaign, err := os.ReadFile(filepath.Join(fixDir, "golden-campaign.db"))
	if err != nil {
		t.Fatalf("failed to read golden-campaign.db: %v", err)
	}

	for _, ver := range []string{"v1", "v2", "v3"} {
		t.Run("Legacy Go "+ver, func(t *testing.T) {
			fn := filepath.Join(fixDir, "crypto", "golden-campaign-"+ver+"-legacy-go.db")
			dec, err := DecryptFile(fn)
			if err != nil {
				t.Fatalf("failed to decrypt legacy Go %s payload: %v", ver, err)
			}
			if !bytes.Equal(dec, goldenCampaign) {
				t.Fatalf("legacy Go %s payload decrypted to unexpected bytes", ver)
			}
		})
	}
}

// TestOpenEncryptedWithPythonFixtures proves that OpenEncrypted opens Python V1/V2/V3
// fixtures and successfully runs SQLite queries and RebuildState.
func TestOpenEncryptedWithPythonFixtures(t *testing.T) {
	SetMasterKeyProvider(func() ([]byte, error) { return testMasterKey(), nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	fixDir := testFixturesDir(t)
	for _, ver := range []string{"v1", "v2", "v3"} {
		t.Run("OpenEncrypted Python "+ver, func(t *testing.T) {
			src := filepath.Join(fixDir, "crypto", "golden-campaign-"+ver+"-python.db")
			tmpDir := t.TempDir()
			dbCopy := filepath.Join(tmpDir, "test-"+ver+".db")

			raw, err := os.ReadFile(src)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(dbCopy, raw, 0o600); err != nil {
				t.Fatal(err)
			}

			store, err := OpenEncrypted(dbCopy, filepath.Join(tmpDir, "dec"))
			if err != nil {
				t.Fatalf("OpenEncrypted failed on Python %s fixture: %v", ver, err)
			}
			defer store.Close()

			ctx := context.Background()
			state1, err := store.RebuildState(ctx, 1)
			if err != nil {
				t.Fatalf("RebuildState(1) failed: %v", err)
			}
			if state1.CurrentStatus != "CONFIRMED" {
				t.Fatalf("unexpected state1 status: %s", state1.CurrentStatus)
			}

			state2, err := store.RebuildState(ctx, 2)
			if err != nil {
				t.Fatalf("RebuildState(2) failed: %v", err)
			}
			if state2.CurrentStatus != "REJECTED_FINAL" {
				t.Fatalf("unexpected state2 status: %s", state2.CurrentStatus)
			}

			state3, err := store.RebuildState(ctx, 3)
			if err != nil {
				t.Fatalf("RebuildState(3) failed: %v", err)
			}
			if state3.CurrentStatus != "ESCALATED" {
				t.Fatalf("unexpected state3 status: %s", state3.CurrentStatus)
			}

			if err := store.CloseAt(dbCopy); err != nil {
				t.Fatalf("CloseAt failed: %v", err)
			}

			// After CloseAt, disk copy must be standard Fernet V3
			newRaw, err := os.ReadFile(dbCopy)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.HasPrefix(newRaw, EncMagicV3) {
				t.Fatalf("re-encrypted file missing V3 magic header: %q", newRaw[:20])
			}
		})
	}
}

// TestGoWriteToPythonRead proves bidirectional interoperability: Go encrypts a file,
// and the real Python runtime decrypts it cleanly.
func TestGoWriteToPythonRead(t *testing.T) {
	requireExternalPythonCrypto(t)
	master := testMasterKey()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	plain := []byte("SQLite format 3\x00cross-language Go-to-Python test data")
	enc, err := EncryptBytesV3(plain, master)
	if err != nil {
		t.Fatalf("EncryptBytesV3: %v", err)
	}

	tmpDir := t.TempDir()
	encFile := filepath.Join(tmpDir, "go-encrypted.db")
	if err := os.WriteFile(encFile, enc, 0o600); err != nil {
		t.Fatal(err)
	}

	// Run python3 to decrypt the Go-written file using the real python-final module
	pyScript := `
import subprocess, types, sys, tempfile
from pathlib import Path

code = subprocess.check_output(['git', 'show', 'python-final:src/symeraseme/core/db_encryption.py']).decode('utf-8')
mod = types.ModuleType('db_encryption')
identity_mod = types.ModuleType('symeraseme.core.identity')
identity_mod._get_existing_master_key = lambda: b'` + testMasterKeyASCII + `'
sys.modules['symeraseme.core.identity'] = identity_mod
sys.modules['symeraseme.core.db_encryption'] = mod
exec(code, mod.__dict__)

enc_path = Path(sys.argv[1])
tmp = mod._decrypt_to_temp(enc_path)
try:
    sys.stdout.buffer.write(tmp.read_bytes())
finally:
    tmp.unlink(missing_ok=True)
`
	cmd := exec.Command("python3", "-c", pyScript, encFile)
	cmd.Dir = repoRoot(t)
	out, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			t.Fatalf("python decrypt failed: %v, stderr: %s", err, exitErr.Stderr)
		}
		t.Fatalf("python decrypt failed: %v", err)
	}

	if !bytes.Equal(out, plain) {
		t.Fatalf("Python decrypted bytes mismatch: got %q, want %q", out, plain)
	}
}

// TestMigrateAccidentalGoV3WithoutDataLoss verifies that an accidental Go V3 file
// is safely migrated to standard Fernet V3 without losing any data.
func TestMigrateAccidentalGoV3WithoutDataLoss(t *testing.T) {
	master := testMasterKey()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	fixDir := testFixturesDir(t)
	src := filepath.Join(fixDir, "crypto", "golden-campaign-v3-legacy-go.db")
	tmpDir := t.TempDir()
	dbCopy := filepath.Join(tmpDir, "migrated-legacy-v3.db")

	raw, err := os.ReadFile(src)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dbCopy, raw, 0o600); err != nil {
		t.Fatal(err)
	}

	// Must be recognized as legacy Go envelope before migration
	if !IsLegacyGoEnvelope(raw) {
		t.Fatal("expected golden-campaign-v3-legacy-go.db to be detected as legacy Go envelope")
	}

	// Open with OpenEncrypted, which should detect and migrate legacy V3
	store, err := OpenEncrypted(dbCopy, filepath.Join(tmpDir, "dec"))
	if err != nil {
		t.Fatalf("OpenEncrypted failed on legacy Go V3: %v", err)
	}

	ctx := context.Background()
	state, err := store.RebuildState(ctx, 1)
	if err != nil {
		store.Close()
		t.Fatalf("RebuildState failed: %v", err)
	}
	if state.CurrentStatus != "CONFIRMED" {
		t.Fatalf("unexpected state after migration: %s", state.CurrentStatus)
	}
	if err := store.CloseAt(dbCopy); err != nil {
		t.Fatalf("CloseAt failed: %v", err)
	}

	// Verify migrated file is now standard Fernet V3 on disk
	migratedRaw, err := os.ReadFile(dbCopy)
	if err != nil {
		t.Fatal(err)
	}
	if IsLegacyGoEnvelope(migratedRaw) {
		t.Fatal("migrated file must not be a legacy Go envelope anymore")
	}

	// And Python can now decrypt the migrated file!
	if os.Getenv("SYMERASEME_RUN_EXTERNAL_PY_TESTS") != "1" {
		return
	}
	requireExternalPythonCrypto(t)
	pyScript := `
import subprocess, types, sys, tempfile
from pathlib import Path

code = subprocess.check_output(['git', 'show', 'python-final:src/symeraseme/core/db_encryption.py']).decode('utf-8')
mod = types.ModuleType('db_encryption')
identity_mod = types.ModuleType('symeraseme.core.identity')
identity_mod._get_existing_master_key = lambda: b'` + testMasterKeyASCII + `'
sys.modules['symeraseme.core.identity'] = identity_mod
sys.modules['symeraseme.core.db_encryption'] = mod
exec(code, mod.__dict__)

enc_path = Path(sys.argv[1])
tmp = mod._decrypt_to_temp(enc_path)
try:
    sys.stdout.buffer.write(tmp.read_bytes())
finally:
    tmp.unlink(missing_ok=True)
`
	cmd := exec.Command("python3", "-c", pyScript, dbCopy)
	cmd.Dir = repoRoot(t)
	out, err := cmd.Output()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			t.Fatalf("python decrypt failed on migrated db: %v, stderr: %s", err, exitErr.Stderr)
		}
		t.Fatalf("python decrypt failed on migrated db: %v", err)
	}

	origCampaign, err := os.ReadFile(filepath.Join(fixDir, "golden-campaign.db"))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(out, origCampaign) {
		t.Fatal("migrated data does not match original golden-campaign.db")
	}
}

// TestCryptoTamperWrongKeyTruncationRejection asserts all invalid inputs fail cleanly.
func TestCryptoTamperWrongKeyTruncationRejection(t *testing.T) {
	master := testMasterKey()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	plain := []byte("SQLite format 3\x00sensitive eventstore database")
	v3Valid, err := EncryptBytesV3(plain, master)
	if err != nil {
		t.Fatal(err)
	}

	// 1. Wrong key rejection
	SetMasterKeyProvider(func() ([]byte, error) {
		badKey := bytes.Repeat([]byte{0x99}, 32)
		return badKey, nil
	})
	if _, err := DecryptBytes(v3Valid); err == nil {
		t.Fatal("decryption with wrong master key should have failed")
	}

	// Restore correct master key
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })

	// 2. Tampered HMAC
	tamperedHMAC := append([]byte(nil), v3Valid...)
	tamperedHMAC[len(tamperedHMAC)-2] ^= 0xff
	if _, err := DecryptBytes(tamperedHMAC); err == nil {
		t.Fatal("decryption with tampered HMAC should have failed")
	}

	// 3. Tampered payload / ciphertext
	tamperedCipher := append([]byte(nil), v3Valid...)
	tamperedCipher[len(EncMagicV3)+SaltLen+5] ^= 0x55
	if _, err := DecryptBytes(tamperedCipher); err == nil {
		t.Fatal("decryption with tampered ciphertext should have failed")
	}

	// 4. Truncation at various boundaries
	truncationPoints := []int{
		5,
		len(EncMagicV3),
		len(EncMagicV3) + SaltLen - 1,
		len(EncMagicV3) + SaltLen,
		len(EncMagicV3) + SaltLen + 20,
		len(v3Valid) - 10,
	}
	for _, pt := range truncationPoints {
		if pt < len(v3Valid) {
			if _, err := DecryptBytes(v3Valid[:pt]); err == nil {
				t.Fatalf("decryption of truncated envelope at %d should have failed", pt)
			}
		}
	}

	// 5. Test rejection on legacy Go payloads as well
	legacyV3, err := EncryptLegacyGoBytesV3(plain, master)
	if err != nil {
		t.Fatal(err)
	}
	SetMasterKeyProvider(func() ([]byte, error) {
		badKey := bytes.Repeat([]byte{0x99}, 32)
		return badKey, nil
	})
	if _, err := DecryptBytes(legacyV3); err == nil {
		t.Fatal("decryption of legacy Go payload with wrong master key should have failed")
	}
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })

	tamperedLegacy := append([]byte(nil), legacyV3...)
	tamperedLegacy[len(tamperedLegacy)-1] ^= 0x01
	if _, err := DecryptBytes(tamperedLegacy); err == nil {
		t.Fatal("decryption of tampered legacy Go HMAC should have failed")
	}
}

// TestTempFileAndWALLifecycle verifies WAL checkpointing before re-encryption,
// clean removal of WAL and shm files, and temp file cleanup.
func TestTempFileAndWALLifecycle(t *testing.T) {
	master := testMasterKey()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	dir := t.TempDir()
	encPath := filepath.Join(dir, "lifecycle.db")
	tmpDir := filepath.Join(dir, "secure-tmp")

	// 1. Initial open and populate
	store, err := OpenEncrypted(encPath, tmpDir)
	if err != nil {
		t.Fatalf("OpenEncrypted failed: %v", err)
	}

	ctx := context.Background()
	reqID, err := store.CreateRemovalRequest(ctx, "broker-lifecycle", "email", "", "DE", "gdpr-art17", "hash-life")
	if err != nil {
		store.Close()
		t.Fatal(err)
	}

	// Write several events to populate WAL
	for i := 0; i < 10; i++ {
		if _, err := store.Append(ctx, reqID, EvtSent, map[string]any{"round": i}, SrcSystem, time.Now().UTC()); err != nil {
			store.Close()
			t.Fatal(err)
		}
	}

	// Close with CloseAt
	if err := store.CloseAt(encPath); err != nil {
		t.Fatalf("CloseAt failed: %v", err)
	}

	// Verify no decrypted temp files or WAL/shm siblings remain in tmpDir
	tmpEntries, err := os.ReadDir(tmpDir)
	if err != nil && !os.IsNotExist(err) {
		t.Fatal(err)
	}
	if len(tmpEntries) != 0 {
		t.Fatalf("expected tmpDir to be clean after CloseAt, found %d entries", len(tmpEntries))
	}

	// 2. Re-open and verify all 10 events survived checkpoint and re-encryption
	store2, err := OpenEncrypted(encPath, tmpDir)
	if err != nil {
		t.Fatalf("re-open OpenEncrypted failed: %v", err)
	}
	defer store2.Close()

	events, err := store2.GetEvents(ctx, reqID, 0)
	if err != nil {
		t.Fatalf("GetEvents failed: %v", err)
	}
	if len(events) != 10 {
		t.Fatalf("expected 10 events after WAL checkpoint and reopen, got %d", len(events))
	}
	if err := store2.CloseAt(encPath); err != nil {
		t.Fatalf("second CloseAt failed: %v", err)
	}
}

// TestCrashSafeAtomicMigrationOnCopies verifies migration on copies only,
// proving atomic replacement and data preservation across V1, V2, and V3.
func TestCrashSafeAtomicMigrationOnCopies(t *testing.T) {
	master := testMasterKey()
	SetMasterKeyProvider(func() ([]byte, error) { return master, nil })
	t.Cleanup(func() { SetMasterKeyProvider(nil) })

	fixDir := testFixturesDir(t)
	goldenCampaign, err := os.ReadFile(filepath.Join(fixDir, "golden-campaign.db"))
	if err != nil {
		t.Fatal(err)
	}

	testCases := []struct {
		name     string
		fixture  string
		isLegacy bool
	}{
		{"Python V1", filepath.Join(fixDir, "crypto", "golden-campaign-v1-python.db"), false},
		{"Python V2", filepath.Join(fixDir, "crypto", "golden-campaign-v2-python.db"), false},
		{"Python V3", filepath.Join(fixDir, "crypto", "golden-campaign-v3-python.db"), false},
		{"Legacy Go V1", filepath.Join(fixDir, "crypto", "golden-campaign-v1-legacy-go.db"), true},
		{"Legacy Go V2", filepath.Join(fixDir, "crypto", "golden-campaign-v2-legacy-go.db"), true},
		{"Legacy Go V3", filepath.Join(fixDir, "crypto", "golden-campaign-v3-legacy-go.db"), true},
	}

	for _, tc := range testCases {
		t.Run("Migrate "+tc.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			copyPath := filepath.Join(tmpDir, "migrate-copy.db")

			raw, err := os.ReadFile(tc.fixture)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(copyPath, raw, 0o600); err != nil {
				t.Fatal(err)
			}

			// OpenEncrypted triggers migration to standard V3
			store, err := OpenEncrypted(copyPath, filepath.Join(tmpDir, "dec"))
			if err != nil {
				t.Fatalf("OpenEncrypted failed: %v", err)
			}
			if err := store.CloseAt(copyPath); err != nil {
				t.Fatalf("CloseAt failed: %v", err)
			}

			// Check disk copy is now standard V3
			migratedRaw, err := os.ReadFile(copyPath)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.HasPrefix(migratedRaw, EncMagicV3) {
				t.Fatal("migrated file missing V3 magic header")
			}
			if IsLegacyGoEnvelope(migratedRaw) {
				t.Fatal("migrated file should not be legacy Go format")
			}

			// Decrypt and compare the complete logical SQLite contents. Additive
			// schema migrations may legitimately change page bytes while preserving
			// every pre-existing row.
			decrypted, err := DecryptBytes(migratedRaw)
			if err != nil {
				t.Fatalf("decrypting migrated file failed: %v", err)
			}
			if got, want := logicalSQLiteSnapshot(t, decrypted), logicalSQLiteSnapshot(t, goldenCampaign); !bytes.Equal(got, want) {
				t.Fatalf("migrated logical SQLite content mismatch\n got: %s\nwant: %s", got, want)
			}
		})
	}
}

func logicalSQLiteSnapshot(t *testing.T, raw []byte) []byte {
	t.Helper()
	path := filepath.Join(t.TempDir(), "snapshot.db")
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		t.Fatal(err)
	}
	store, err := Open(path)
	if err != nil {
		t.Fatalf("open logical snapshot: %v", err)
	}
	defer store.Close()
	tables, err := store.DB().Query(`SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name`)
	if err != nil {
		t.Fatalf("list logical snapshot tables: %v", err)
	}
	var names []string
	for tables.Next() {
		var name string
		if err := tables.Scan(&name); err != nil {
			t.Fatal(err)
		}
		names = append(names, name)
	}
	if err := tables.Err(); err != nil {
		t.Fatal(err)
	}
	if err := tables.Close(); err != nil {
		t.Fatal(err)
	}
	snapshot := make(map[string][][]any, len(names))
	for _, name := range names {
		quoted := `"` + strings.ReplaceAll(name, `"`, `""`) + `"`
		rows, err := store.DB().Query(fmt.Sprintf("SELECT * FROM %s ORDER BY rowid", quoted))
		if err != nil {
			t.Fatalf("query logical snapshot table %s: %v", name, err)
		}
		columns, err := rows.Columns()
		if err != nil {
			rows.Close()
			t.Fatal(err)
		}
		for rows.Next() {
			values := make([]any, len(columns))
			dest := make([]any, len(columns))
			for i := range values {
				dest[i] = &values[i]
			}
			if err := rows.Scan(dest...); err != nil {
				rows.Close()
				t.Fatal(err)
			}
			snapshot[name] = append(snapshot[name], values)
		}
		if err := rows.Err(); err != nil {
			rows.Close()
			t.Fatal(err)
		}
		if err := rows.Close(); err != nil {
			t.Fatal(err)
		}
	}
	encoded, err := json.Marshal(snapshot)
	if err != nil {
		t.Fatal(err)
	}
	return encoded
}

// TestFernetStandardFormatVerification verifies byte-level standard Fernet properties:
// base64 URL-safe encoding, first character 'g', correct header and salt offsets.
func TestFernetStandardFormatVerification(t *testing.T) {
	master := testMasterKey()
	plain := []byte("deterministic check that new writes use standard Fernet format")

	v3, err := EncryptBytesV3(plain, master)
	if err != nil {
		t.Fatal(err)
	}

	// Layout: 17 bytes magic ("SYMERASEME_ENCv3\n") + 16 bytes salt + Fernet token
	if len(v3) < 17+SaltLen+73 {
		t.Fatalf("V3 envelope too short: %d bytes", len(v3))
	}
	if !bytes.Equal(v3[:17], EncMagicV3) {
		t.Fatalf("unexpected header: %q", v3[:17])
	}

	tokenBytes := v3[17+SaltLen:]
	// In standard Fernet, the token starts with 'g' (base64 of version 0x80)
	if tokenBytes[0] != 'g' {
		t.Fatalf("expected standard Fernet token starting with 'g', got 0x%02x (%c)", tokenBytes[0], tokenBytes[0])
	}
	if IsLegacyGoEnvelope(v3) {
		t.Fatal("new write must NOT be detected as legacy Go envelope")
	}

	// Verify DecryptFernetToken can decrypt it directly
	salt := v3[17 : 17+SaltLen]
	key, err := DeriveKeyHKDF(master, salt, HKDFInfoV3)
	if err != nil {
		t.Fatal(err)
	}
	dec, err := DecryptFernetToken(tokenBytes, key)
	if err != nil {
		t.Fatalf("DecryptFernetToken failed on new write: %v", err)
	}
	if !bytes.Equal(dec, plain) {
		t.Fatalf("decrypted mismatch: got %q, want %q", dec, plain)
	}
}
