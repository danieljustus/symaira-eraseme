// Command crypto is a test-only cross-language V3 crypto oracle.
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
		if len(input) < 33+len(eventstore.EncMagicV3)+eventstore.SaltLen {
			fail("invalid envelope")
		}
		offset := len(eventstore.EncMagicV3) + eventstore.SaltLen
		keyBytes, deriveErr := eventstore.DeriveKeyHKDF(key, input[33+len(eventstore.EncMagicV3):33+offset], eventstore.HKDFInfoV3)
		if deriveErr != nil {
			fail("key derivation failed")
		}
		output, err = eventstore.DecryptFernetToken(input[33+offset:], keyBytes)
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

func fail(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
