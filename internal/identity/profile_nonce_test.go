package identity

import (
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/zalando/go-keyring"
)

func TestLoadProfileRejectsInvalidNonceWithoutSideEffects(t *testing.T) {
	clearKey(t)
	key := testKey(t)
	fake := NewFakeKeyring()
	SetKeyringBackend(fake)

	validRaw, err := EncryptProfileWithKey([]byte(`{}`), key)
	if err != nil {
		t.Fatalf("build synthetic encrypted profile: %v", err)
	}
	separator := bytes.IndexByte(validRaw, '\n')
	if separator < 0 {
		t.Fatal("synthetic encrypted profile has no header separator")
	}
	ciphertext := append([]byte(nil), validRaw[separator+1:]...)

	tests := []struct {
		name      string
		nonce     string
		omitNonce bool
	}{
		{name: "malformed", nonce: "not-hex"},
		{name: "missing", omitNonce: true},
		{name: "empty", nonce: ""},
		{name: "short", nonce: "00"},
		{name: "long", nonce: strings.Repeat("00", NonceLength+1)},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			headerJSON, err := invalidNonceHeader(tt.nonce, tt.omitNonce)
			if err != nil {
				t.Fatalf("build synthetic envelope: %v", err)
			}
			raw := make([]byte, 0, len(headerJSON)+1+len(ciphertext))
			raw = append(raw, headerJSON...)
			raw = append(raw, '\n')
			raw = append(raw, ciphertext...)

			dir := t.TempDir()
			path := filepath.Join(dir, "identity.encrypted")
			if err := os.WriteFile(path, raw, 0o600); err != nil {
				t.Fatalf("write synthetic profile: %v", err)
			}
			beforeRaw, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("read fixture before load: %v", err)
			}
			beforeEntries, err := os.ReadDir(dir)
			if err != nil {
				t.Fatalf("read fixture directory before load: %v", err)
			}
			beforeInfo, err := os.Stat(path)
			if err != nil {
				t.Fatalf("stat fixture before load: %v", err)
			}

			_, loadErr := loadProfileWithoutPanic(t, path)
			if loadErr == nil {
				t.Fatal("LoadProfile unexpectedly accepted invalid nonce")
			}
			if !errors.Is(loadErr, ErrProfileCorrupt) {
				t.Fatalf("LoadProfile error = %v, want ErrProfileCorrupt-compatible error", loadErr)
			}

			afterRaw, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("read fixture after load: %v", err)
			}
			if !bytes.Equal(afterRaw, beforeRaw) {
				t.Fatal("rejected profile changed on-disk contents")
			}
			afterEntries, err := os.ReadDir(dir)
			if err != nil {
				t.Fatalf("read fixture directory after load: %v", err)
			}
			if !reflect.DeepEqual(entryNames(beforeEntries), entryNames(afterEntries)) {
				t.Fatalf("rejected profile changed directory entries: before=%v after=%v", entryNames(beforeEntries), entryNames(afterEntries))
			}
			afterInfo, err := os.Stat(path)
			if err != nil {
				t.Fatalf("stat fixture after load: %v", err)
			}
			if afterInfo.Mode().Perm() != beforeInfo.Mode().Perm() || afterInfo.Size() != beforeInfo.Size() || !afterInfo.ModTime().Equal(beforeInfo.ModTime()) {
				t.Fatal("rejected profile changed on-disk metadata")
			}

			if _, err := fake.Get(ServiceName, KeyringUsername); !errors.Is(err, keyring.ErrNotFound) {
				t.Fatalf("rejected profile touched fake keyring: %v", err)
			}
			profileCacheMu.Lock()
			_, cached := profileCache[filepath.Clean(path)]
			profileCacheMu.Unlock()
			if cached {
				t.Fatal("rejected profile was added to the profile cache")
			}
		})
	}
}

func invalidNonceHeader(nonce string, omitNonce bool) ([]byte, error) {
	if omitNonce {
		return json.Marshal(struct {
			Version   int    `json:"version"`
			Algorithm string `json:"algorithm"`
		}{Version: 2, Algorithm: "AES-256-GCM"})
	}
	return json.Marshal(ProfileEnvelope{Version: 2, Nonce: nonce, Algorithm: "AES-256-GCM"})
}

func loadProfileWithoutPanic(t *testing.T, path string) (profile *Profile, err error) {
	t.Helper()
	defer func() {
		if recovered := recover(); recovered != nil {
			t.Fatalf("LoadProfile panicked: %v", recovered)
		}
	}()
	return LoadProfile(path)
}

func entryNames(entries []os.DirEntry) []string {
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	return names
}
