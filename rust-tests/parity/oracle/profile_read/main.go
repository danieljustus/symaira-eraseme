// Test-only adapter copied into an archive of the pinned production Go tree.
package main

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

type observation struct {
	Exists   bool              `json:"exists"`
	Profile  *identity.Profile `json:"profile"`
	Class    string            `json:"class"`
	Error    string            `json:"error"`
	KeyReads int               `json:"key_reads"`
}
type testCase struct {
	Name        string            `json:"name"`
	Path        string            `json:"path"`
	Environment map[string]string `json:"environment"`
	Files       map[string][]byte `json:"files"`
	Directories []string          `json:"directories"`
	Key         string            `json:"key"`
	Expected    observation       `json:"expected"`
}
type fakeKeyring struct{ reads int }

func (k *fakeKeyring) Get(_, _ string) (string, error) {
	k.reads++
	return "", errors.New("test missing")
}
func (*fakeKeyring) Set(_, _, _ string) error { panic("unexpected key write") }
func (*fakeKeyring) Delete(_, _ string) error { panic("unexpected key delete") }

func must(err error) {
	if err != nil {
		panic(err)
	}
}
func classify(err error) string {
	if err == nil {
		return "ok"
	}
	switch {
	case errors.Is(err, identity.ErrProfileNotFound):
		return "not_found"
	case errors.Is(err, identity.ErrMasterKeyMissing):
		return "key_missing"
	case errors.Is(err, identity.ErrLegacyV0Unsupported):
		return "legacy_v0"
	case strings.Contains(err.Error(), "no header separator"):
		return "separator"
	case strings.Contains(err.Error(), ": header:"):
		return "header"
	case strings.Contains(err.Error(), ": nonce:") || strings.Contains(err.Error(), "invalid nonce length"):
		return "nonce"
	case strings.Contains(err.Error(), "message authentication failed"):
		return "authentication"
	case errors.Is(err, identity.ErrProfileCorrupt):
		return "json"
	case strings.HasPrefix(err.Error(), "identity: stat profile:"):
		return "stat"
	case strings.HasPrefix(err.Error(), "identity: read profile:"):
		return "read"
	default:
		panic(err)
	}
}
func snapshot(root string) map[string]string {
	result := map[string]string{}
	must(filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, e := filepath.Rel(root, path)
		if e != nil {
			return e
		}
		info, e := d.Info()
		if e != nil {
			return e
		}
		value := info.Mode().String()
		if !d.IsDir() {
			raw, e := os.ReadFile(path)
			if e != nil {
				return e
			}
			value += string(raw)
		}
		result[rel] = value
		return nil
	}))
	return result
}
func execute(c testCase) (result observation) {
	root, err := os.MkdirTemp("", "profile-read-")
	must(err)
	defer os.RemoveAll(root)
	for _, name := range []string{"HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "SYMERASEME_IDENTITY_PATH", "SYMERASEME_DATA_DIR", "SYMERASEME_CONFIG_DIR", "SYMERASEME_IDENTITY_MASTER_KEY", "SYMVAULT_PASSPHRASE"} {
		must(os.Unsetenv(name))
	}
	must(os.Setenv("HOME", root))
	must(os.Setenv("USERPROFILE", root))
	for name, value := range c.Environment {
		must(os.Setenv(name, strings.ReplaceAll(value, "$ROOT", root)))
	}
	for _, dir := range c.Directories {
		must(os.MkdirAll(filepath.Join(root, dir), 0700))
	}
	for name, raw := range c.Files {
		target := filepath.Join(root, name)
		must(os.MkdirAll(filepath.Dir(target), 0700))
		must(os.WriteFile(target, raw, 0600))
	}
	before, _ := json.Marshal(snapshot(root))
	keyring := &fakeKeyring{}
	defer func() {
		result.KeyReads = keyring.reads
		after, _ := json.Marshal(snapshot(root))
		if !bytes.Equal(before, after) {
			panic("loader wrote filesystem")
		}
	}()
	defer func() {
		if value := recover(); value != nil {
			panic(fmt.Sprintf("unexpected panic in case %s: %v", c.Name, value))
		}
	}()
	identity.SetKeyringBackend(keyring)
	key := bytes.Repeat([]byte{0x42}, 32)
	if c.Key == "wrong" {
		key = bytes.Repeat([]byte{0x43}, 32)
	}
	if c.Key == "missing" {
		key = nil
	}
	must(identity.SetMasterKey(key))
	path := strings.ReplaceAll(c.Path, "$ROOT", root)
	result.Exists = identity.ProfileExists(path)
	result.Profile, err = identity.LoadProfile(path)
	result.Class = classify(err)
	if err != nil {
		result.Error = strings.ReplaceAll(err.Error(), root, "$ROOT")
	}
	return result
}
func cases() []testCase {
	key := bytes.Repeat([]byte{0x42}, 32)
	encryptBytes := func(plain []byte) []byte {
		out, err := identity.EncryptProfileWithKey(plain, key)
		must(err)
		return out
	}
	encrypt := func(plain string) []byte {
		return encryptBytes([]byte(plain))
	}
	full := encrypt(`{"full_name":"TEST Alice Ä","name_variants":["TEST A"],"date_of_birth":"2000-02-03","addresses":[{"street":"TEST Street","city":"TEST City","postal_code":"00000","country":"DE","state":"TEST State","valid_from":"2020-01-01","valid_to":null}],"email_addresses":["test@example.invalid"],"phone_numbers":["TEST Phone"],"jurisdictions":["GDPR"]}`)
	result := []testCase{}
	add := func(name string, raw []byte) {
		result = append(result, testCase{Name: name, Path: "$ROOT/identity.encrypted", Files: map[string][]byte{"identity.encrypted": raw}, Environment: map[string]string{}, Directories: []string{}, Key: "normal"})
	}
	add("full", full)
	// Authenticated variant headers are synthetic inputs, sealed by Go's AES
	// primitive, then read by the unchanged production LoadProfile below.
	block, err := aes.NewCipher(key)
	must(err)
	gcm, err := cipher.NewGCM(block)
	must(err)
	for _, version := range []int{1, -7, 3, 2} {
		header := fmt.Sprintf(`{"version":%d,"nonce":"070707070707070707070707","algorithm":"TEST ignored","extra":true}`, version)
		raw := append([]byte(header+"\n"), gcm.Seal(nil, bytes.Repeat([]byte{7}, 12), []byte(`{"full_name":"TEST variant"}`), []byte(header))...)
		add(fmt.Sprintf("authenticated-version-%d", version), raw)
	}
	add("unicode-field-fold", encrypt(`{"addresses":[{"ſtate":"TEST State"}]}`))
	add("short-nonce", []byte("{\"version\":2,\"nonce\":\"00\"}\nTEST"))
	add("empty-object", encrypt(`{}`))
	add("null-profile", encrypt(`null`))
	add("normalization", encrypt(`{"full_name":null,"name_variants":null,"addresses":[null,{}],"email_addresses":[null,"test@example.invalid"],"phone_numbers":null,"jurisdictions":null}`))
	add("case-and-duplicates", encrypt(`{"FULL_NAME":"TEST First","full_name":null,"Full_Name":"TEST Last","addresses":[{"CITY":"TEST First","city":null,"City":"TEST Last"}],"unknown":{"anything":true}}`))
	add("empty-file", []byte{})
	add("missing-separator", []byte("TEST malformed"))
	add("bad-header", []byte("{\nTEST"))
	add("null-header", []byte("null\nTEST"))
	add("v0", []byte("{\"version\":0}\nTEST"))
	add("header-type", []byte("{\"version\":\"TEST\"}\nTEST"))
	add("invalid-json", encrypt(`{"full_name":`))
	add("invalid-field", encrypt(`{"full_name":42}`))
	add("invalid-array-element", encrypt(`{"addresses":[42]}`))
	add("invalid-root", encrypt(`[]`))
	add("trailing-json", encrypt(`{} {}`))
	add("trailing-comma-object", encrypt(`{"full_name":"TEST Alice",}`))
	add("trailing-comma-address", encrypt(`{"addresses":[{"city":"TEST City",}]}`))
	add("trailing-comma-address-array", encrypt(`{"addresses":[{"city":"TEST City"},]}`))
	add("trailing-comma-strings-array", encrypt(`{"email_addresses":["test@example.invalid",]}`))
	add("trailing-comma-unknown-object", encrypt(`{"unknown":{"key":"value",}}`))
	add("trailing-comma-unknown-array", encrypt(`{"unknown":[1, 2,]}`))
	add("invalid-utf8-full-name", encryptBytes([]byte("{\"full_name\":\"TEST \xff\"}")))
	add("invalid-utf8-sequence", encryptBytes([]byte("{\"email_addresses\":[\"TEST \xc0\xaf\"]}")))
	add("lone-high-surrogate", encrypt(`{"full_name":"\ud800"}`))
	add("lone-low-surrogate", encrypt(`{"full_name":"\udc00"}`))
	add("valid-surrogate-pair", encrypt(`{"full_name":"\ud83d\ude00"}`))
	deepJSON := func(depth int) []byte {
		var builder strings.Builder
		builder.Grow(depth + 16)
		builder.WriteString(`{"deep":`)
		for index := 1; index < depth; index++ {
			builder.WriteByte('[')
		}
		builder.WriteString("null")
		for index := 1; index < depth; index++ {
			builder.WriteByte(']')
		}
		builder.WriteByte('}')
		return []byte(builder.String())
	}
	for _, depth := range []int{127, 128, 9999, 10000, 10001} {
		add(fmt.Sprintf("unknown-depth-%d", depth), encryptBytes(deepJSON(depth)))
	}
	add("empty-plaintext", encrypt(``))
	corrupt := append([]byte(nil), full...)
	corrupt[len(corrupt)-1] ^= 1
	add("tampered-ciphertext", corrupt)
	headerEnd := bytes.IndexByte(full, '\n')
	add("truncated-ciphertext", full[:headerEnd+1])
	add("tampered-aad", bytes.Replace(full, []byte(`"version":2`), []byte(`"version":3`), 1))
	add("bad-nonce-hex", []byte("{\"version\":2,\"nonce\":\"zz\"}\nTEST"))
	add("wrong-key", full)
	result[len(result)-1].Key = "wrong"
	add("missing-key", full)
	result[len(result)-1].Key = "missing"
	add("malformed-before-key", []byte{})
	result[len(result)-1].Key = "missing"
	add("absent", nil)
	result[len(result)-1].Files = map[string][]byte{}
	add("legacy-fallback", full)
	result[len(result)-1].Files = map[string][]byte{"identity.enc": full}
	add("canonical-priority", full)
	result[len(result)-1].Files["identity.enc"] = []byte("invalid")
	add("corrupt-canonical-no-fallback", []byte{})
	result[len(result)-1].Files["identity.enc"] = full
	add("explicit-legacy-alternate", full)
	result[len(result)-1].Path = "$ROOT/identity.enc"
	add("custom-no-fallback", full)
	result[len(result)-1].Path = "$ROOT/custom"
	add("directory", nil)
	result[len(result)-1].Files = map[string][]byte{}
	result[len(result)-1].Directories = []string{"identity.encrypted"}
	add("parent-is-file", full)
	result[len(result)-1].Path = "$ROOT/identity.encrypted/child"
	for _, entry := range []struct {
		name, path string
		env        map[string]string
		file       string
	}{
		{"default-config", "", map[string]string{}, ".config/symeraseme/identity.encrypted"},
		{"default-legacy", "", map[string]string{}, ".config/symeraseme/identity.enc"},
		{"data-over-config", "", map[string]string{"SYMERASEME_DATA_DIR": "$ROOT/data", "SYMERASEME_CONFIG_DIR": "$ROOT/config"}, "data/identity.encrypted"},
		{"identity-over-data", "", map[string]string{"SYMERASEME_DATA_DIR": "$ROOT/data", "SYMERASEME_IDENTITY_PATH": "$ROOT/custom"}, "custom"},
		{"config-override", "", map[string]string{"SYMERASEME_CONFIG_DIR": "$ROOT/config"}, "config/identity.encrypted"},
		{"tilde-override", "", map[string]string{"SYMERASEME_IDENTITY_PATH": "~/custom"}, "custom"},
		{"tilde-explicit", "~/custom", map[string]string{}, "custom"},
		{"explicit-over-env", "$ROOT/custom", map[string]string{"SYMERASEME_IDENTITY_PATH": "$ROOT/missing"}, "custom"},
	} {
		result = append(result, testCase{Name: entry.name, Path: entry.path, Environment: entry.env, Files: map[string][]byte{entry.file: full}, Directories: []string{}, Key: "normal"})
	}
	return result
}
func main() {
	input := cases()
	if len(os.Args) > 1 && os.Args[1] == "replay" {
		must(json.NewDecoder(os.Stdin).Decode(&input))
	}
	for i := range input {
		input[i].Expected = execute(input[i])
	}
	raw, err := json.MarshalIndent(input, "", "  ")
	must(err)
	fmt.Println(string(raw))
}
