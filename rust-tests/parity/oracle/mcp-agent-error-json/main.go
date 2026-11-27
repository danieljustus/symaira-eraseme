// Command mcp-agent-error-json records Go's stdio MCP encoding for malformed
// UTF-8 returned by the host-agent process.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

const agentScript = "#!/bin/sh\nprintf '%b' \"$AGENT_STDERR_ESCAPED\" >&2\nexit 23\n"
const agentStderrEscaped = `before\360\200\200after\377end`
const requestFrame = `{"jsonrpc":"2.0","id":91,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1,"save":false}}}`

type fixture struct {
	Schema                 string            `json:"schema"`
	GoVersion              string            `json:"go_version"`
	SourcesSHA256          map[string]string `json:"sources_sha256"`
	AgentScriptSHA256      string            `json:"agent_script_sha256"`
	AgentStderrEscaped     string            `json:"agent_stderr_escaped"`
	AgentStderrBytesBase64 string            `json:"agent_stderr_bytes_base64"`
	RequestBase64          string            `json:"request_base64"`
	ResponseBase64         string            `json:"response_base64"`
}

func main() {
	writeFixture := flag.Bool("fixture", false, "write the generated fixture")
	flag.Parse()

	root, err := os.MkdirTemp("", "eraseme-mcp-agent-error-json-")
	must(err)
	defer os.RemoveAll(root)
	data := filepath.Join(root, "data")
	bin := filepath.Join(root, "bin")
	must(os.MkdirAll(data, 0700))
	must(os.MkdirAll(bin, 0700))
	must(os.WriteFile(filepath.Join(bin, "claude"), []byte(agentScript), 0700))
	for name, value := range map[string]string{
		"HOME":                     root,
		"PATH":                     bin,
		"SYMERASEME_DATA_DIR":      data,
		"SYMERASEME_LLM_PROVIDER":  "agent",
		"SYMERASEME_AGENT_BACKEND": "claude",
		"AGENT_STDERR_ESCAPED":     agentStderrEscaped,
	} {
		must(os.Setenv(name, value))
	}

	store, err := eventstore.Open(filepath.Join(data, "symeraseme.db"))
	must(err)
	requestID, err := store.CreateRemovalRequest(context.Background(), "oracle-broker", "email", "oracle-campaign", "DE", "gdpr-art17.de.md.j2", "")
	must(err)
	_, err = replies.NewRepository(store).InsertReply(context.Background(), &requestID, "oracle-message", "oracle-thread", "privacy@example.invalid", "We need your current address", "Your current address does not match our records.", "")
	must(err)
	must(store.Close())

	request := []byte(requestFrame)
	var response bytes.Buffer
	err = mcp.NewServer(mcp.ContractHandler()).ServeStdio(
		context.Background(), bytes.NewReader(append(request, '\n')), &response,
	)
	must(err)

	result := fixture{
		Schema:                 "symeraseme.go-oracle.mcp-agent-error-json.v1",
		GoVersion:              runtime.Version(),
		SourcesSHA256:          make(map[string]string),
		AgentScriptSHA256:      digest([]byte(agentScript)),
		AgentStderrEscaped:     agentStderrEscaped,
		AgentStderrBytesBase64: base64.StdEncoding.EncodeToString([]byte{0x62, 0x65, 0x66, 0x6f, 0x72, 0x65, 0xf0, 0x80, 0x80, 0x61, 0x66, 0x74, 0x65, 0x72, 0xff, 0x65, 0x6e, 0x64}),
		RequestBase64:          base64.StdEncoding.EncodeToString(append(request, '\n')),
		ResponseBase64:         base64.StdEncoding.EncodeToString(response.Bytes()),
	}
	for _, path := range []string{
		"go.mod",
		"internal/mcp/server.go",
		"internal/mcp/contract_handler.go",
		"internal/llm/agent.go",
		"internal/llm/llm.go",
		"internal/triage/classifier.go",
		"internal/eventstore/store.go",
		"rust-tests/parity/oracle/mcp-agent-error-json/main.go",
	} {
		contents, err := os.ReadFile(path)
		must(err)
		result.SourcesSHA256[path] = digest(contents)
	}
	encoded, err := json.MarshalIndent(result, "", "  ")
	must(err)
	encoded = append(encoded, '\n')
	if *writeFixture {
		must(os.MkdirAll("tests/fixtures/mcp-agent-error-json", 0755))
		must(os.WriteFile("tests/fixtures/mcp-agent-error-json/error.json", encoded, 0644))
		fmt.Fprintln(os.Stderr, "wrote tests/fixtures/mcp-agent-error-json/error.json")
		return
	}
	_, err = os.Stdout.Write(encoded)
	must(err)
}

func digest(contents []byte) string {
	sum := sha256.Sum256(contents)
	return hex.EncodeToString(sum[:])
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
