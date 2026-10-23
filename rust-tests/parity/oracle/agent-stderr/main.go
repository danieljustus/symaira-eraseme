// Command agent-stderr records Go host-agent stderr trimming and byte-limit behavior.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

type result struct {
	Schema             string            `json:"schema"`
	GoVersion          string            `json:"go_version"`
	SourcesSHA256      map[string]string `json:"sources_sha256"`
	ID                 string            `json:"id"`
	StderrPrintfEscape string            `json:"stderr_printf_escape"`
	Error              string            `json:"error"`
	ErrorType          string            `json:"error_type"`
}

func main() {
	out := flag.String("fixture", "tests/fixtures/agent-stderr/invalid-utf8.json", "fixture output path")
	flag.Parse()

	root, err := os.MkdirTemp("", "eraseme-agent-stderr-oracle-")
	fatalIf(err)
	defer os.RemoveAll(root)
	bin := filepath.Join(root, "bin")
	fatalIf(os.Mkdir(bin, 0o700))
	cli := filepath.Join(bin, "claude")
	program := "#!/bin/sh\nprintf '%b' \"$AGENT_STDERR_ESCAPED\" >&2\nexit 23\n"
	fatalIf(os.WriteFile(cli, []byte(program), 0o700))

	var stderr strings.Builder
	stderr.WriteString(" \\t")
	for range 260 {
		stderr.WriteString("e\\377")
	}
	stderr.WriteString("\\t\\n")

	fatalIf(os.Setenv("PATH", bin))
	fatalIf(os.Setenv("AGENT_STDERR_ESCAPED", stderr.String()))
	fatalIf(os.Setenv("TERM", "oracle-term"))
	client := llm.NewAgentClient("auto", "claude", nil)
	client.MaxRetries = 1
	_, _, callErr := client.Classify(context.Background(), "system", "user", llm.ClassifyOptions{})
	if callErr == nil {
		fatalIf(fmt.Errorf("fake host agent unexpectedly succeeded"))
	}

	fixture := result{
		Schema:             "symeraseme.go-oracle.agent-stderr.v1",
		GoVersion:          runtime.Version(),
		SourcesSHA256:      map[string]string{},
		ID:                 "invalid-utf8-is-truncated-after-go-space-trim",
		StderrPrintfEscape: stderr.String(),
		Error:              callErr.Error(),
		ErrorType:          fmt.Sprintf("%T", callErr),
	}
	for _, source := range []string{"internal/llm/agent.go", "go.mod"} {
		contents, err := os.ReadFile(source)
		fatalIf(err)
		digest := sha256.Sum256(contents)
		fixture.SourcesSHA256[source] = hex.EncodeToString(digest[:])
	}
	encoded, err := json.MarshalIndent(fixture, "", "  ")
	fatalIf(err)
	fatalIf(os.MkdirAll(filepath.Dir(*out), 0o755))
	fatalIf(os.WriteFile(*out, append(encoded, '\n'), 0o644))
	fmt.Printf("recorded %s with %s -> %s\n", fixture.ID, fixture.GoVersion, *out)
}

func fatalIf(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
