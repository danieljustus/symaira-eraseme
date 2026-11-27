package eventstore

import (
	"bytes"
	"context"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// TestEncryptV3RoundTrip: a store opened via OpenEncrypted, written and
// closed, produces a V3-encrypted file that re-opens with the same master
// key and returns the same data (encrypt-on-close / decrypt-on-open).
func TestEncryptV3RoundTrip(t *testing.T) {
	dir := t.TempDir()
	encPath := filepath.Join(dir, "campaign.db")
	tmpDir := filepath.Join(dir, "tmp")
	master := "unit-test-master-key"

	ctx := context.Background()
	ts, err := time.Parse(time.RFC3339, "2026-08-01T08:05:00Z")
	if err != nil {
		t.Fatal(err)
	}

	SetMasterKeyProvider(func() ([]byte, error) { return []byte(pad32(master)), nil })
	s, err := OpenEncrypted(encPath, tmpDir)
	if err != nil {
		t.Fatalf("open encrypted: %v", err)
	}
	if _, err := s.CreateRemovalRequest(ctx, "broker-y", "email", "", "DE", "gdpr-art17", "hash-y"); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Append(ctx, 1, EvtSent, map[string]any{"recipient": "a@b.example.com"}, SrcSystem, ts); err != nil {
		t.Fatal(err)
	}
	if err := s.CloseAt(encPath); err != nil {
		t.Fatalf("close with encrypt-on-close: %v", err)
	}

	// The file on disk must now carry the V3 envelope.
	raw, err := os.ReadFile(encPath)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(raw, []byte("SYMERASEME_ENCv3\n")) {
		t.Fatalf("expected V3 magic header, got prefix %q", raw[:min(len(raw), 20)])
	}
	if v, ok := DetectVersion(raw); !ok || v != 3 {
		t.Fatal("DetectVersion must report 3 for the written envelope")
	}

	// Re-open with the same key: data must be readable.
	s2, err := OpenEncrypted(encPath, tmpDir)
	if err != nil {
		t.Fatalf("reopen encrypted: %v", err)
	}
	defer s2.Close()
	state, err := s2.RebuildState(ctx, 1)
	if err != nil {
		t.Fatalf("rebuild from decrypted db: %v", err)
	}
	if state.CurrentStatus != "AWAITING_ACK" || state.SentAt == nil || *state.SentAt != "2026-08-01T08:05:00+00:00" {
		t.Errorf("round-trip state mismatch: %+v", state)
	}
}

// TestEncryptWrongKeyFails: a different master key must not silently
// produce readable data.
func TestEncryptWrongKeyFails(t *testing.T) {
	dir := t.TempDir()
	encPath := filepath.Join(dir, "campaign.db")
	tmpDir := filepath.Join(dir, "tmp")

	SetMasterKeyProvider(func() ([]byte, error) { return []byte(pad32("key-one")), nil })
	s, err := OpenEncrypted(encPath, tmpDir)
	if err != nil {
		t.Fatal(err)
	}
	if err := s.CloseAt(encPath); err != nil {
		t.Fatal(err)
	}

	SetMasterKeyProvider(func() ([]byte, error) { return []byte(pad32("key-two")), nil })
	s2, err := OpenEncrypted(encPath, tmpDir)
	if err != nil {
		// Either OpenEncrypted fails outright or the first query does —
		// both are acceptable rejections.
		return
	}
	defer s2.Close()
	if _, err := s2.RebuildState(context.Background(), 1); err == nil {
		t.Fatal("reading with the wrong master key must fail")
	}
}

// TestDetectVersionRejectsPlaintext: a plain SQLite file must not be
// mistaken for an encrypted envelope.
func TestDetectVersionRejectsPlaintext(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "plain.db")
	s, err := Open(p)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.CreateRemovalRequest(context.Background(), "b", "email", "", "US", "ccpa-deletion", "h"); err != nil {
		t.Fatal(err)
	}
	s.Close()
	raw, err := os.ReadFile(p)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := DetectVersion(raw); ok {
		t.Fatal("plaintext SQLite must not be detected as encrypted")
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// pad32 stretches a test key to the required 32 bytes deterministically.
func pad32(s string) string {
	out := make([]byte, 32)
	copy(out, s)
	for i := len(s); i < 32; i++ {
		out[i] = 'x'
	}
	return string(out)
}

// TestEncryptionHeaderContract pins exact raw V1/V2/V3 header bytes and
// length 17 (issue #795).
func TestEncryptionHeaderContract(t *testing.T) {
	cases := []struct {
		name      string
		version   int
		magic     []byte
		wantBytes []byte
		wantHex   string
		wantLen   int
	}{
		{
			name:      "V1",
			version:   1,
			magic:     EncHeaderV1,
			wantBytes: []byte("SYMERASEME_ENCv1\n"),
			wantHex:   "53594d45524153454d455f454e4376310a",
			wantLen:   17,
		},
		{
			name:      "V2",
			version:   2,
			magic:     EncMagicV2,
			wantBytes: []byte("SYMERASEME_ENCv2\n"),
			wantHex:   "53594d45524153454d455f454e4376320a",
			wantLen:   17,
		},
		{
			name:      "V3",
			version:   3,
			magic:     EncMagicV3,
			wantBytes: []byte("SYMERASEME_ENCv3\n"),
			wantHex:   "53594d45524153454d455f454e4376330a",
			wantLen:   17,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if len(tc.magic) != tc.wantLen {
				t.Errorf("%s runtime header has length %d, want %d", tc.name, len(tc.magic), tc.wantLen)
			}
			if !bytes.Equal(tc.magic, tc.wantBytes) {
				t.Errorf("%s runtime header bytes = %q, want %q", tc.name, tc.magic, tc.wantBytes)
			}
			if got := hex.EncodeToString(tc.magic); got != tc.wantHex {
				t.Errorf("%s hex = %s, want %s", tc.name, got, tc.wantHex)
			}

			// Verify DetectVersion correctly identifies the header
			v, ok := DetectVersion(tc.magic)
			if !ok {
				t.Errorf("%s DetectVersion failed on exact header bytes", tc.name)
			} else if v != tc.version {
				t.Errorf("%s DetectVersion = %d, want %d", tc.name, v, tc.version)
			}
		})
	}

	// Verify byte offsets in encrypted V2/V3 envelopes:
	// Envelope layout: [17 bytes header] + [16 bytes salt] + [Fernet token ...]
	master := bytes.Repeat([]byte{0x5a}, 32)
	payload := []byte("payload for header offset test")

	v2Env, err := EncryptBytesV2(payload, master)
	if err != nil {
		t.Fatalf("EncryptBytesV2: %v", err)
	}
	if len(v2Env) < 17+SaltLen {
		t.Fatalf("v2 envelope too short: %d bytes", len(v2Env))
	}
	if !bytes.Equal(v2Env[:17], EncMagicV2) {
		t.Errorf("v2 envelope header mismatch: %q", v2Env[:17])
	}

	v3Env, err := EncryptBytesV3(payload, master)
	if err != nil {
		t.Fatalf("EncryptBytesV3: %v", err)
	}
	if len(v3Env) < 17+SaltLen {
		t.Fatalf("v3 envelope too short: %d bytes", len(v3Env))
	}
	if !bytes.Equal(v3Env[:17], EncMagicV3) {
		t.Errorf("v3 envelope header mismatch: %q", v3Env[:17])
	}
}
