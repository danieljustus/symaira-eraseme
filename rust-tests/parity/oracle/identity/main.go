// Command identity is a test-only cross-language identity profile & crypto oracle.
// It accepts a request on stdin:
//   op == 'e': 1 byte 'e' + 32-byte key + plaintext JSON. Encrypts with Go and writes envelope bytes to stdout.
//   op == 'd': 1 byte 'd' + 32-byte key + envelope bytes. Decrypts with Go and writes plaintext JSON to stdout.
//   op == 'c': 1 byte 'c' + profile JSON. Parses profile and writes CanonicalJSON bytes to stdout.
//   op == 'h': 1 byte 'h' + profile JSON. Parses profile and writes HashProfile hex string to stdout.
//   op == 's': 1 byte 's' + 32-byte key + 4-byte BE path len + path + profile JSON.
//              Calls SetMasterKey, SaveProfile. Writes resolved path to stdout.
//   op == 'l': 1 byte 'l' + 32-byte key + path.
//              Calls SetMasterKey, LoadProfile, writes CanonicalJSON of loaded profile to stdout.
package main

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func main() {
	input, err := io.ReadAll(os.Stdin)
	if err != nil || len(input) < 1 {
		fail("invalid request: empty stdin")
	}
	op := input[0]
	rest := input[1:]

	var output []byte
	switch op {
	case 'e':
		if len(rest) < 32 {
			fail("invalid request: short key")
		}
		key := rest[:32]
		plain := rest[32:]
		output, err = identity.EncryptProfileWithKey(plain, key)
	case 'd':
		if len(rest) < 32 {
			fail("invalid request: short key")
		}
		key := rest[:32]
		raw := rest[32:]
		output, err = identity.DecryptProfileWithKey(raw, key)
	case 'c':
		var p identity.Profile
		if err = json.Unmarshal(rest, &p); err != nil {
			fail("unmarshal profile failed: " + err.Error())
		}
		output = identity.CanonicalJSON(&p)
	case 'h':
		var p identity.Profile
		if err = json.Unmarshal(rest, &p); err != nil {
			fail("unmarshal profile failed: " + err.Error())
		}
		hash := identity.HashProfile(&p)
		output = []byte(hash)
	case 's':
		if len(rest) < 36 {
			fail("invalid request for save: too short")
		}
		key := rest[:32]
		pathLen := binary.BigEndian.Uint32(rest[32:36])
		if len(rest) < 36+int(pathLen) {
			fail("invalid request: path truncated")
		}
		path := string(rest[36 : 36+pathLen])
		payload := rest[36+pathLen:]
		var p identity.Profile
		if err = json.Unmarshal(payload, &p); err != nil {
			fail("unmarshal profile failed: " + err.Error())
		}
		if err = identity.SetMasterKey(key); err != nil {
			fail("set master key failed: " + err.Error())
		}
		target, saveErr := identity.SaveProfile(&p, path)
		if saveErr != nil {
			fail("save profile failed: " + saveErr.Error())
		}
		output = []byte(target)
	case 'l':
		if len(rest) < 32 {
			fail("invalid request for load: too short")
		}
		key := rest[:32]
		path := string(rest[32:])
		if err = identity.SetMasterKey(key); err != nil {
			fail("set master key failed: " + err.Error())
		}
		p, loadErr := identity.LoadProfile(path)
		if loadErr != nil {
			fail("load profile failed: " + loadErr.Error())
		}
		output = identity.CanonicalJSON(p)
	default:
		fail(fmt.Sprintf("unknown operation: %c", op))
	}

	if err != nil {
		fail("operation failed: " + err.Error())
	}
	if _, err := os.Stdout.Write(output); err != nil {
		fail("write stdout failed: " + err.Error())
	}
}

func fail(msg string) {
	fmt.Fprintln(os.Stderr, msg)
	os.Exit(1)
}
