// Native executable fixture for private Go/Rust triage process differentials.
// It records only invocation counts, never prompts, credentials or operator data.
package main

import (
	"fmt"
	"os"
	"strings"
)

func main() {
	if path := os.Getenv("SYMERASEME_NATIVE_AGENT_CONTROL"); path != "" {
		file, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0600)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(2)
		}
		_, err = file.WriteString("invoked\n")
		closeErr := file.Close()
		if err != nil || closeErr != nil {
			os.Exit(2)
		}
	}
	if os.Getenv("AGENT_STDERR_ESCAPED") != "" {
		_, _ = os.Stderr.Write([]byte{'b', 'e', 'f', 'o', 'r', 'e', 0xff, 'a', 'f', 't', 'e', 'r'})
		os.Exit(23)
	}
	args := strings.Join(os.Args[1:], " ")
	switch {
	case strings.Contains(args, "--model oracle-env-model"):
		fmt.Println(`{"classification":"confirmed","confidence":0.93,"summary":"model selected from environment","extracted_fields":{"ticket":"T-42"}}`)
	case strings.Contains(args, "--model oracle-flag-model"):
		fmt.Println(`{"classification":"confirmed","confidence":0.93,"summary":"model selected from flag","extracted_fields":{"ticket":"T-42"}}`)
	case strings.Contains(args, "rejection classifier"):
		fmt.Println(`{"classification":"address_mismatch","confidence":0.91,"summary":"address differs","key_points":[],"jurisdiction":"GDPR"}`)
	case strings.Contains(args, "email classifier"):
		fmt.Println(`{"classification":"confirmed","confidence":0.93,"summary":"deletion confirmed","extracted_fields":{"ticket":"T-42"}}`)
	default:
		fmt.Println(`{"classification":"other","confidence":0.1,"summary":"unexpected prompt","key_points":[],"jurisdiction":"unknown"}`)
	}
}
