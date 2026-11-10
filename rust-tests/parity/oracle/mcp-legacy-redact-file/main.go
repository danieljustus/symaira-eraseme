// Command mcp-legacy-redact-file is the byte oracle for the bare legacy
// `redact_file` JSON-RPC method.
//
// It is a separate program from `mcp-tools-call` on purpose: the bare method is
// a different code path in `internal/mcp/server.go` (its own `case`, its own
// `legacyPath` argument reader) and it answers differently from `tools/call` —
// the result is NOT wrapped in the content envelope and a handler failure is
// `-32602`, not `-32603`. Keeping the family separate leaves the byte-pinned
// `mcp-003` fixtures untouched.
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
	sourcePath     = "internal/mcp/server.go:237-265"
	fixtureDir     = "tests/fixtures/mcp-contract/mcp-006"
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

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

func writeFixture() {
	workspace, err := os.MkdirTemp("", "mcp-legacy-redact-file")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(workspace)

	// The handler treats the process working directory as its workspace root,
	// so the cases use relative paths and the recording runs from the
	// workspace. Same file content as the `tools/call` family, so the redaction
	// itself is comparable across the two code paths.
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
			Name:    "bare_redact_file_reads_object_params",
			Request: `{"jsonrpc":"2.0","id":1,"method":"redact_file","params":{"path":"pii.txt"}}`,
		},
		{
			Name:    "bare_redact_file_reads_positional_params",
			Request: `{"jsonrpc":"2.0","id":2,"method":"redact_file","params":["pii.txt"]}`,
		},
		{
			Name:    "bare_redact_file_leaves_clean_text_unchanged",
			Request: `{"jsonrpc":"2.0","id":3,"method":"redact_file","params":{"path":"clean.txt"}}`,
		},
		{
			Name:    "bare_redact_file_rejects_object_params_without_a_path",
			Request: `{"jsonrpc":"2.0","id":4,"method":"redact_file","params":{}}`,
		},
		{
			Name:    "bare_redact_file_rejects_an_empty_object_path",
			Request: `{"jsonrpc":"2.0","id":5,"method":"redact_file","params":{"path":""}}`,
		},
		{
			Name:    "bare_redact_file_rejects_an_empty_positional_path",
			Request: `{"jsonrpc":"2.0","id":6,"method":"redact_file","params":[""]}`,
		},
		{
			Name:    "bare_redact_file_rejects_a_non_string_positional_path",
			Request: `{"jsonrpc":"2.0","id":7,"method":"redact_file","params":[42]}`,
		},
		{
			Name:    "bare_redact_file_rejects_a_nested_positional_path",
			Request: `{"jsonrpc":"2.0","id":8,"method":"redact_file","params":[["pii.txt"]]}`,
		},
		{
			Name:    "bare_redact_file_rejects_missing_params",
			Request: `{"jsonrpc":"2.0","id":9,"method":"redact_file"}`,
		},
		{
			Name:    "bare_redact_file_rejects_a_non_string_object_path",
			Request: `{"jsonrpc":"2.0","id":10,"method":"redact_file","params":{"path":42}}`,
		},
		{
			// The distinguishing case: a handler failure on the bare method is
			// `-32602` with the sanitized message, while `tools/call` answers
			// `-32603` for the same failure.
			Name:    "bare_redact_file_reports_a_handler_failure_as_invalid_params",
			Request: `{"jsonrpc":"2.0","id":11,"method":"redact_file","params":{"path":"missing.txt"}}`,
		},
		{
			// A notification carries no id, so the server must answer nothing.
			Name:    "bare_redact_file_notification_stays_silent",
			Request: `{"jsonrpc":"2.0","method":"redact_file","params":{"path":"pii.txt"}}`,
		},
	}

	recorded := make([]fixtureCase, 0, len(cases))
	for _, testCase := range cases {
		record := fixtureCase{Name: testCase.Name, Request: testCase.Request}
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandler())
		runErr := server.ServeStdio(
			context.Background(),
			bytes.NewReader(append([]byte(testCase.Request), '\n')),
			&output,
		)
		if runErr != nil {
			record.ParseError = true
		} else if body := output.String(); body != "" {
			record.Response = &body
		}
		recorded = append(recorded, record)
	}

	target, err := os.Create(filepath.Join(origin, fixtureDir, "cases.json"))
	if err != nil {
		fail(err)
	}
	defer target.Close()
	encoder := json.NewEncoder(target)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Cases          []fixtureCase `json:"cases"`
	}{
		SourceRevision: sourceRevision,
		SourcePath:     sourcePath,
		Cases:          recorded,
	}); err != nil {
		fail(err)
	}
}
