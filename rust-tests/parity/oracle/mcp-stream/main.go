// Command mcp-stream is the byte oracle for MCP stdio stream framing.
//
// Go's ServeStdio decodes consecutive JSON values from the stream, so framing
// is whitespace-separated JSON values rather than lines. Each case therefore
// defines the complete stream and no terminator is appended.
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
	sourceRevision = "4cdbf9be02f76f4384a0eb0c8fdbe37b3aef19f7"
	sourcePath     = "internal/mcp/server.go:377-430"
)

const (
	initialise   = `{"jsonrpc":"2.0","id":1,"method":"initialize"}`
	second       = `{"jsonrpc":"2.0","id":2,"method":"initialize"}`
	notification = `{"jsonrpc":"2.0","method":"initialize"}`
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
	if err := mcp.NewServer(nil).ServeStdio(context.Background(), bytes.NewReader(raw), &output); err != nil {
		fail(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(struct {
		BodyB64 string `json:"body_b64"`
	}{BodyB64: base64.StdEncoding.EncodeToString(output.Bytes())}); err != nil {
		fail(err)
	}
}

func writeFixture() {
	cases := []fixtureCase{
		{Name: "two_values_separated_by_newline", raw: []byte(initialise + "\n" + second + "\n")},
		{Name: "two_values_separated_by_crlf", raw: []byte(initialise + "\r\n" + second + "\r\n")},
		{Name: "three_values_adjacent_without_whitespace", raw: []byte(initialise + initialise + second)},
		{Name: "value_split_across_a_newline", raw: []byte("{\"jsonrpc\":\"2.0\",\n\"id\":1,\"method\":\"initialize\"}\n")},
		{Name: "leading_whitespace_is_skipped", raw: []byte("  \t\n\r" + initialise + "\n")},
		{Name: "empty_input_produces_nothing", raw: []byte("")},
		{Name: "whitespace_only_input_produces_nothing", raw: []byte(" \n\t\r\n")},
		{Name: "notification_then_request", raw: []byte(notification + "\n" + initialise + "\n")},
		{Name: "scalar_value_is_an_invalid_request", raw: []byte("1\n")},
		{Name: "string_value_is_an_invalid_request", raw: []byte("\"x\"\n")},
		{Name: "truncated_final_value_is_an_error", raw: []byte(initialise + "\n" + `{"jsonrpc":"2.0",`)},
		{Name: "junk_after_a_value_is_an_error", raw: []byte(initialise + " @@@\n")},
		{Name: "malformed_first_value_is_an_error", raw: []byte("nope\n" + initialise + "\n")},
	}
	for index := range cases {
		cases[index].RequestB64 = base64.StdEncoding.EncodeToString(cases[index].raw)
		var output bytes.Buffer
		err := mcp.NewServer(nil).ServeStdio(context.Background(), bytes.NewReader(cases[index].raw), &output)
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
