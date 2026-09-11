// Command crypto is a test-only cross-language V3 crypto oracle.
// It accepts a binary request on stdin: one operation byte ('e' or 'd'),
// a 32-byte master key, then plaintext (e) or an envelope (d). It writes
// only the binary result to stdout and never accepts secrets in argv.
package main

import (
	"bytes"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

const maxRequestBytes = 64 << 20

func main() {
	input, err := io.ReadAll(io.LimitReader(os.Stdin, maxRequestBytes+1))
	if err != nil || len(input) > maxRequestBytes || len(input) < 33 {
		fail("invalid request")
	}
	key := input[1:33]
	var output []byte
	switch input[0] {
	case 'e':
		output, err = eventstore.EncryptBytesV3(input[33:], key)
	case 'd':
		envelope := input[33:]
		if len(envelope) < len(eventstore.EncMagicV3)+eventstore.SaltLen ||
			!bytes.HasPrefix(envelope, eventstore.EncMagicV3) {
			fail("invalid envelope")
		}
		offset := len(eventstore.EncMagicV3) + eventstore.SaltLen
		keyBytes, deriveErr := eventstore.DeriveKeyHKDF(key, envelope[len(eventstore.EncMagicV3):offset], eventstore.HKDFInfoV3)
		if deriveErr != nil {
			fail("key derivation failed")
		}
		output, err = eventstore.DecryptFernetToken(envelope[offset:], keyBytes)
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
