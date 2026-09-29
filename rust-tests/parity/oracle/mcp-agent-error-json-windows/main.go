// Command mcp-agent-error-json-windows is both the Go MCP oracle and a native
// fake host-agent executable. Copy it to claude.exe (claude on Unix) to let
// exec.LookPath and exec.Command exercise the real process path.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

var agentStderr = []byte{'b', 'e', 'f', 'o', 'r', 'e', 0xf0, 0x80, 0x80, 'a', 'f', 't', 'e', 'r', 0xff, 'e', 'n', 'd'}

const requestFrame = `{"jsonrpc":"2.0","id":91,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1,"save":false}}}`

type observation struct {
	Schema                 string `json:"schema"`
	Platform               string `json:"platform"`
	AgentStderrBytesBase64 string `json:"agent_stderr_bytes_base64"`
	RequestBase64          string `json:"request_base64"`
	ResponseBase64         string `json:"response_base64"`
}

func main() {
	if len(os.Args) == 3 && os.Args[1] == "--flood" {
		output := os.Stdout
		if os.Args[2] == "stderr" {
			output = os.Stderr
		}
		_, _ = output.Write(bytes.Repeat([]byte{'x'}, 2*1024*1024))
		time.Sleep(30 * time.Second)
		return
	}
	if len(os.Args) == 2 && os.Args[1] == "--oracle" {
		if err := runOracle(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	_, _ = os.Stderr.Write(agentStderr)
	os.Exit(23)
}

func runOracle() error {
	data := os.Getenv("SYMERASEME_DATA_DIR")
	if data == "" {
		return fmt.Errorf("SYMERASEME_DATA_DIR is required")
	}
	store, err := eventstore.Open(filepath.Join(data, "symeraseme.db"))
	if err != nil {
		return err
	}
	requestID, err := store.CreateRemovalRequest(context.Background(), "oracle-broker", "email", "oracle-campaign", "DE", "gdpr-art17.de.md.j2", "")
	if err != nil {
		return err
	}
	_, err = replies.NewRepository(store).InsertReply(context.Background(), &requestID, "oracle-message", "oracle-thread", "privacy@example.invalid", "We need your current address", "Your current address does not match our records.", "")
	if err != nil {
		return err
	}
	if err := store.Close(); err != nil {
		return err
	}

	request := append([]byte(requestFrame), '\n')
	var response bytes.Buffer
	if err := mcp.NewServer(mcp.ContractHandler()).ServeStdio(context.Background(), bytes.NewReader(request), &response); err != nil {
		return err
	}
	encoded, err := json.Marshal(observation{
		Schema:                 "symeraseme.go-oracle.mcp-agent-error-json-windows.v1",
		Platform:               runtime.GOOS + "/" + runtime.GOARCH,
		AgentStderrBytesBase64: base64.StdEncoding.EncodeToString(agentStderr),
		RequestBase64:          base64.StdEncoding.EncodeToString(request),
		ResponseBase64:         base64.StdEncoding.EncodeToString(response.Bytes()),
	})
	if err != nil {
		return err
	}
	_, err = os.Stdout.Write(append(encoded, '\n'))
	return err
}
