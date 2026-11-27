// Command mcp-result is the byte oracle for the MCP tool result envelope and
// for the error sanitization applied to handler failures. It injects a
// deterministic handler per case, because both behaviours are only observable
// through a handler.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const (
	sourceRevision = "2f2e6985be9cf231afba7018c4314ac45193e3cc"
	sourcePath     = "internal/mcp/server.go:38-44,180-250,344-375"
)

const callRequest = `{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"redact_file","arguments":{"path":"x"}}}`

type fixtureCase struct {
	Name       string  `json:"name"`
	Handler    string  `json:"handler"`
	Request    string  `json:"request,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
}

func handlerFor(kind string) mcp.Handler {
	return func(context.Context, string, map[string]any) (any, error) {
		switch kind {
		case "string":
			return "redacted text", nil
		case "object":
			return map[string]any{"b": []any{2}, "a": 1}, nil
		case "array":
			return []any{1, "two"}, nil
		case "number":
			return 42, nil
		case "nil-handler":
			return nil, nil
		case "error-sqlite":
			return nil, errors.New("SQLite error: disk I/O error")
		case "error-no-such-table":
			return nil, errors.New("no such table: removal_requests")
		case "error-locked":
			return nil, errors.New("database is locked")
		case "error-malformed":
			return nil, errors.New("database disk image is malformed")
		case "error-panic-marker":
			return nil, errors.New("panic: runtime error: index out of range")
		case "error-plain":
			return nil, errors.New("campaign not found")
		}
		return nil, fmt.Errorf("unknown handler kind %q", kind)
	}
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
	server := mcp.NewServer(handlerFor("string"))
	if err := server.ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output); err != nil {
		fail(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(struct {
		BodyB64 string `json:"body_b64"`
	}{BodyB64: base64.StdEncoding.EncodeToString(output.Bytes())}); err != nil {
		fail(err)
	}
}

func writeFixture() {
	kinds := []struct{ name, handler string }{
		{"result_envelope_wraps_string_result", "string"},
		{"result_envelope_wraps_object_result", "object"},
		{"result_envelope_wraps_array_result", "array"},
		{"result_envelope_wraps_number_result", "number"},
		{"handler_returning_nil_result", "nil-handler"},
		{"sanitized_sqlite_error_hides_storage_detail", "error-sqlite"},
		{"sanitized_no_such_table_error_hides_storage_detail", "error-no-such-table"},
		{"sanitized_locked_error_hides_storage_detail", "error-locked"},
		{"sanitized_malformed_image_error_hides_storage_detail", "error-malformed"},
		{"sanitized_panic_error_becomes_internal_server_error", "error-panic-marker"},
		{"unsanitized_handler_error_is_passed_through", "error-plain"},
	}
	cases := make([]fixtureCase, 0, len(kinds)+1)
	for _, kind := range kinds {
		cases = append(cases, fixtureCase{Name: kind.name, Handler: kind.handler, Request: callRequest})
	}
	cases = append(cases, fixtureCase{Name: "default_handler_reports_missing_backend", Handler: "default-nil", Request: callRequest})
	for index := range cases {
		var server *mcp.Server
		if cases[index].Handler == "default-nil" {
			server = mcp.NewServer(nil)
		} else {
			server = mcp.NewServer(handlerFor(cases[index].Handler))
		}
		var output bytes.Buffer
		raw := []byte(cases[index].Request)
		err := server.ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output)
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
