// Command cry006 captures the pinned Go encryption oracle's observable
// decryption errors for the CRY006 parity matrix. It delegates all cryptography
// to internal/eventstore; standard raw cases are decoded from genuine standard
// fixtures and are never constructed by this harness.
package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

const (
	pinnedCommit = "bf53346eec234929bedf0314b99e3da85dbb991b"
	masterKey    = "symaira-eraseme-golden-master-32"
)

type result struct {
	ID       string `json:"id"`
	Version  int    `json:"version"`
	Fixture  string `json:"fixture"`
	Outcome  string `json:"outcome"`
	Error    string `json:"error,omitempty"`
	PlainSHA string `json:"plaintext_sha256,omitempty"`
}

type capture struct {
	OracleCommit    string            `json:"oracle_commit"`
	OracleSourceSHA string            `json:"oracle_source_sha256"`
	Generator       string            `json:"generator"`
	GeneratorSHA256 string            `json:"generator_sha256"`
	Cases           []result          `json:"cases"`
	FixtureSHA256   map[string]string `json:"fixture_sha256"`
}

func sha(data []byte) string {
	digest := sha256.Sum256(data)
	return hex.EncodeToString(digest[:])
}

func standardRaw(envelope []byte, headerLen, saltLen int) []byte {
	offset := headerLen + saltLen
	decoded, err := base64.URLEncoding.DecodeString(string(envelope[offset:]))
	if err != nil {
		panic(fmt.Sprintf("standard fixture token is not padded URL-safe base64: %v", err))
	}
	out := append([]byte(nil), envelope[:offset]...)
	return append(out, decoded...)
}

func mutate(raw []byte, kind string, headerLen, saltLen int) []byte {
	out := append([]byte(nil), raw...)
	switch kind {
	case "malformed_header":
		copy(out[:headerLen], []byte("SYMERASEME_ENCv9\n"))
	case "short":
		out = out[:headerLen]
	case "short_token":
		out = out[:headerLen+saltLen]
	case "hmac_tamper", "gcm_failure":
		offset := headerLen + saltLen
		if saltLen == 0 {
			offset = headerLen
		}
		out[offset+len(out[offset:])/2] ^= 1
	case "raw_standard_tamper":
		offset := headerLen + saltLen
		out[offset+30] ^= 1
	case "wrong_key", "valid", "raw_standard_valid":
		// handled by the caller or already represented by the source bytes
	}
	return out
}

func main() {
	root, err := os.Getwd()
	if err != nil {
		panic(err)
	}
	fixDir := filepath.Join(root, "tests", "fixtures", "event-store", "crypto")
	key := []byte(masterKey)
	source, err := os.ReadFile(filepath.Join(root, "internal", "eventstore", "encrypt.go"))
	if err != nil {
		panic(fmt.Errorf("read executed oracle source: %w", err))
	}
	generator, err := os.ReadFile(filepath.Join(root, "rust-tests", "parity", "oracle", "cry006", "main.go"))
	if err != nil {
		panic(fmt.Errorf("read executed generator: %w", err))
	}
	out := capture{
		OracleCommit:    pinnedCommit,
		OracleSourceSHA: sha(source),
		Generator:       "rust-tests/parity/oracle/cry006/main.go",
		GeneratorSHA256: sha(generator),
		FixtureSHA256:   map[string]string{},
	}
	fixtureNames := []string{
		"golden-campaign-v1-legacy-go.db", "golden-campaign-v2-legacy-go.db", "golden-campaign-v3-legacy-go.db",
		"golden-campaign-v1-python.db", "golden-campaign-v2-python.db", "golden-campaign-v3-python.db",
	}
	for _, name := range fixtureNames {
		data, err := os.ReadFile(filepath.Join(fixDir, name))
		if err != nil {
			panic(err)
		}
		out.FixtureSHA256[name] = sha(data)
	}
	for _, tc := range []struct {
		version int
		name    string
		header  int
		salt    int
	}{
		{1, "golden-campaign-v1-legacy-go.db", len(eventstore.EncHeaderV1), 0},
		{2, "golden-campaign-v2-legacy-go.db", len(eventstore.EncMagicV2), eventstore.SaltLen},
		{3, "golden-campaign-v3-legacy-go.db", len(eventstore.EncMagicV3), eventstore.SaltLen},
	} {
		raw, err := os.ReadFile(filepath.Join(fixDir, tc.name))
		if err != nil {
			panic(err)
		}
		for _, kind := range []string{"valid", "malformed_header", "short", "short_token", "wrong_key", "hmac_tamper", "gcm_failure"} {
			input := raw
			useKey := key
			if kind != "valid" && kind != "wrong_key" {
				input = mutate(raw, kind, tc.header, tc.salt)
			}
			if kind == "wrong_key" {
				useKey = make([]byte, 32)
			}
			appendResult(&out, tc.version, tc.name, fmt.Sprintf("v%d_%s", tc.version, kind), input, useKey)
		}

		standardName := fmt.Sprintf("golden-campaign-v%d-python.db", tc.version)
		standard, err := os.ReadFile(filepath.Join(fixDir, standardName))
		if err != nil {
			panic(err)
		}
		for _, kind := range []string{"raw_standard_valid", "raw_standard_tamper"} {
			input := standardRaw(standard, tc.header, tc.salt)
			if kind == "raw_standard_tamper" {
				input = mutate(input, kind, tc.header, tc.salt)
			}
			appendResult(&out, tc.version, standardName, fmt.Sprintf("v%d_%s", tc.version, kind), input, key)
		}
	}
	encoded, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		panic(err)
	}
	fmt.Println(string(encoded))
}

func appendResult(out *capture, version int, fixture, id string, input, key []byte) {
	plain, decErr := decrypt(input, key)
	item := result{ID: id, Version: version, Fixture: fixture, Outcome: "error"}
	if decErr == nil {
		item.Outcome = "success"
		item.PlainSHA = sha(plain)
	} else {
		item.Error = decErr.Error()
	}
	out.Cases = append(out.Cases, item)
}

func decrypt(raw, key []byte) ([]byte, error) {
	eventstore.SetMasterKeyProvider(func() ([]byte, error) { return key, nil })
	return eventstore.DecryptBytes(raw)
}
