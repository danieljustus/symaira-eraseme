// Command mcp-envelope is the byte oracle for the MCP JSON-RPC envelope,
// notification and tools/call validation contract.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const (
	sourceRevision = "a51c7f3c65218924ce1d505ad8389b2216f08c92"
	sourcePath     = "internal/mcp/server.go:180-251,377-389"
)

type fixtureCase struct {
	Name       string  `json:"name"`
	Request    string  `json:"request,omitempty"`
	RequestB64 string  `json:"request_b64,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
	raw        []byte  `json:"-"`
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
	if err := mcp.NewServer(nil).ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output); err != nil {
		fail(err)
	}
	result := struct {
		BodyB64 string `json:"body_b64"`
	}{BodyB64: base64.StdEncoding.EncodeToString(output.Bytes())}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fail(err)
	}
}

func writeFixture() {
	cases := []fixtureCase{
		{Name: "notification_unknown_method_is_silent", Request: `{"jsonrpc":"2.0","method":"nope"}`},
		{Name: "notification_unknown_method_with_params_is_silent", Request: `{"jsonrpc":"2.0","method":"nope","params":{}}`},
		{Name: "request_unknown_method_is_not_found", Request: `{"jsonrpc":"2.0","id":1,"method":"nope"}`},
		{Name: "empty_object_is_invalid_request", Request: `{}`},
		{Name: "empty_batch_is_invalid_request", Request: `[]`},
		{Name: "batch_array_initialize", Request: `[{"jsonrpc":"2.0","id":1,"method":"initialize"}]`},
		{Name: "tools_call_missing_name", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}`},
		{Name: "tools_call_blank_name", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"  "}}`},
		{Name: "tools_call_unknown_tool", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nope"}}`},
		{Name: "tools_call_missing_required_argument", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"redact_file"}}`},
		{Name: "tools_call_non_object_arguments", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"redact_file","arguments":[]}}`},
		{Name: "tools_call_non_object_params", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":[]}`},
		{Name: "tools_call_invalid_parameter_type", Request: `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"plan_show","arguments":{"campaign_id":5}}}`},
		{Name: "legacy_bare_status_is_not_found", Request: `{"jsonrpc":"2.0","id":1,"method":"status"}`},
	}
	for index := range cases {
		raw := []byte(cases[index].Request)
		if cases[index].raw != nil {
			raw = cases[index].raw
			cases[index].RequestB64 = base64.StdEncoding.EncodeToString(raw)
		}
		var output bytes.Buffer
		err := mcp.NewServer(nil).ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output)
		if err != nil {
			cases[index].ParseError = true
		} else if body := output.String(); body != "" {
			cases[index].Response = &body
		}
	}
	output := struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Cases          []fixtureCase `json:"cases"`
	}{SourceRevision: sourceRevision, SourcePath: sourcePath, Cases: cases}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(output); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
