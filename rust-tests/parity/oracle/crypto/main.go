// Command crypto is a test-only cross-language crypto oracle.
// It accepts a binary request on stdin: one operation byte ('e' or 'd'),
// a 32-byte master key, then plaintext (e) or an envelope (d). It writes
// only the binary result to stdout and never accepts secrets in argv.
package main

import (
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func main() {
	input, err := io.ReadAll(os.Stdin)
	if err != nil || len(input) < 33 {
		fail("invalid request")
	}
	key := input[1:33]
	var output []byte
	switch input[0] {
	case 'e':
		output, err = eventstore.EncryptBytesV3(input[33:], key)
	case 'd':
		output, err = decryptEnvelope(input[33:], key)
	default:
		fail("unknown operation")
	}
	if err != nil {
		fail("crypto operation failed")
	}
	if _, err := os.Stdout.Write(output); err != nil {
		fail("write failed")
	}
}

// decryptEnvelope validates the complete legacy envelope framing before
// deriving a key or slicing the token. Keeping version-specific offsets here
// makes malformed headers, truncated salts, and unsupported versions fail via
// the same secret-safe oracle error path as cryptographic failures.
func decryptEnvelope(envelope, masterKey []byte) ([]byte, error) {
	version, ok := eventstore.DetectVersion(envelope)
	if !ok {
		return nil, fmt.Errorf("invalid encryption envelope")
	}

	var (
		tokenStart int
		salt       []byte
		key        []byte
	)
	switch version {
	case 1:
		tokenStart = len(eventstore.EncHeaderV1)
		salt = eventstore.PBKDF2FixedSalt
		key = eventstore.DeriveKeyPBKDF2(masterKey, salt)
	case 2:
		tokenStart = len(eventstore.EncMagicV2) + eventstore.SaltLen
		if len(envelope) < tokenStart {
			return nil, fmt.Errorf("truncated encryption envelope")
		}
		salt = envelope[len(eventstore.EncMagicV2):tokenStart]
		key = eventstore.DeriveKeyPBKDF2(masterKey, salt)
	case 3:
		tokenStart = len(eventstore.EncMagicV3) + eventstore.SaltLen
		if len(envelope) < tokenStart {
			return nil, fmt.Errorf("truncated encryption envelope")
		}
		salt = envelope[len(eventstore.EncMagicV3):tokenStart]
		var deriveErr error
		key, deriveErr = eventstore.DeriveKeyHKDF(masterKey, salt, eventstore.HKDFInfoV3)
		if deriveErr != nil {
			return nil, fmt.Errorf("key derivation failed")
		}
	default:
		return nil, fmt.Errorf("unsupported encryption version")
	}
	if len(envelope) <= tokenStart {
		return nil, fmt.Errorf("truncated encryption token")
	}
	return eventstore.DecryptFernetToken(envelope[tokenStart:], key)
}

func fail(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
