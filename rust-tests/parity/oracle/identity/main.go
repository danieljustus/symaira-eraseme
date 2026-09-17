// Command identity is a focused Go oracle for the ID-001 profile wire format.
// Requests are binary on stdin: e|d + 32-byte key + payload, or c|g|h + JSON.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func main() {
	input, err := io.ReadAll(os.Stdin)
	if err != nil || len(input) == 0 {
		fail("invalid request")
	}

	var output []byte
	switch input[0] {
	case 'e':
		if len(input) < 33 {
			fail("short encryption request")
		}
		output, err = identity.EncryptProfileWithKey(input[33:], input[1:33])
	case 'd':
		if len(input) < 33 {
			fail("short decryption request")
		}
		output, err = identity.DecryptProfileWithKey(input[33:], input[1:33])
	case 'c', 'g', 'h':
		if input[0] == 'g' {
			var value any
			if err = json.Unmarshal(input[1:], &value); err == nil {
				output, err = identity.CanonicalGenericJSON(value)
			}
			break
		}
		var profile identity.Profile
		if err = json.Unmarshal(input[1:], &profile); err == nil {
			if input[0] == 'c' {
				output = identity.CanonicalJSON(&profile)
			} else {
				output = []byte(identity.HashProfile(&profile))
			}
		}
	default:
		fail(fmt.Sprintf("unknown operation %q", input[0]))
	}
	if err != nil {
		fail(err.Error())
	}
	if _, err := os.Stdout.Write(output); err != nil {
		fail(err.Error())
	}
}

func fail(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}
