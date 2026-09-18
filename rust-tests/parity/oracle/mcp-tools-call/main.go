// Command mcp-tools-call is the byte oracle for MCP `tools/call` responses
// produced by the real contract handler.
//
// Unlike the earlier MCP oracles this one deliberately does NOT use a stub
// handler: it wires `mcp.ContractHandler()`, so every recorded response is the
// production answer for the given tool and arguments.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const (
	sourceRevision = "79bf23e83b31f18d98487101200eaf32749e5a46"
	sourcePath     = "internal/mcp/contract_handler.go:167-320"
)

type fixtureCase struct {
	Name       string  `json:"name"`
	Request    string  `json:"request,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		writeFixture()
		return
	}
	raw, err := io.ReadAll(os.Stdin)
	if err != nil {
		fail(err)
	}
	var output bytes.Buffer
	server := mcp.NewServer(mcp.ContractHandler())
	if err := server.ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output); err != nil {
		fail(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(struct {
		BodyB64 string `json:"body_b64"`
	}{BodyB64: base64.StdEncoding.EncodeToString(output.Bytes())}); err != nil {
		fail(err)
	}
}

func callRequest(id int, name string, arguments string) string {
	return fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"%s","arguments":%s}}`, id, name, arguments)
}

func writeFixture() {
	workspace, err := os.MkdirTemp("", "mcp-tools-call")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(workspace)

	// The MCP handler reads with the process working directory as its
	// workspace root, so the fixture uses relative names and runs from the
	// workspace. That keeps every recorded request reproducible.
	if err := os.WriteFile(filepath.Join(workspace, "pii.txt"), []byte("Contact jane.doe@example.com or 555-123-4567.\n"), 0o600); err != nil {
		fail(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "clean.txt"), []byte("No personal data here.\n"), 0o600); err != nil {
		fail(err)
	}
	origin, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	if err := os.Chdir(workspace); err != nil {
		fail(err)
	}
	defer func() { _ = os.Chdir(origin) }()

	cases := []fixtureCase{
		{
			Name:    "redact_file_returns_the_redacted_text",
			Request: callRequest(1, "redact_file", `{"path":"pii.txt"}`),
		},
		{
			Name:    "redact_file_leaves_clean_text_unchanged",
			Request: callRequest(2, "redact_file", `{"path":"clean.txt"}`),
		},
		{
			Name:    "redact_file_reports_a_missing_file_as_a_tool_error",
			Request: callRequest(3, "redact_file", `{"path":"missing.txt"}`),
		},
		{
			// `status` passes validation as the legacy alias but has no case in
			// the contract handler, so Go answers with its default.
			Name:    "legacy_status_alias_hits_the_handler_default",
			Request: callRequest(4, "status", `{}`),
		},
	}

	for index := range cases {
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandler())
		err := server.ServeStdio(context.Background(), bytes.NewReader(append([]byte(cases[index].Request), '\n')), &output)
		if err != nil {
			cases[index].ParseError = true
		} else if body := output.String(); body != "" {
			cases[index].Response = &body
		}
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Cases          []fixtureCase `json:"cases"`
	}{SourceRevision: sourceRevision, SourcePath: sourcePath, Cases: cases}); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
