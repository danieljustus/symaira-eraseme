// Command agent-cancel records Go MCP HTTP cancellation through the host agent.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

type result struct {
	Schema        string            `json:"schema"`
	GoVersion     string            `json:"go_version"`
	SourcesSHA256 map[string]string `json:"sources_sha256"`
	HandlerError  string            `json:"handler_error"`
	ClientError   string            `json:"client_error"`
	ChildExited   bool              `json:"child_exited"`
}

func main() {
	out := flag.String("fixture", "tests/fixtures/agent-cancel/http.json", "fixture output path")
	flag.Parse()

	root, err := os.MkdirTemp("", "eraseme-agent-cancel-oracle-")
	fatalIf(err)
	defer os.RemoveAll(root)
	bin := filepath.Join(root, "bin")
	fatalIf(os.Mkdir(bin, 0o700))
	started := filepath.Join(root, "started")
	cli := filepath.Join(bin, "claude")
	program := "#!/bin/sh\nprintf '%s' \"$$\" > \"$AGENT_STARTED\"\nexec /bin/sleep 30\n"
	fatalIf(os.WriteFile(cli, []byte(program), 0o700))
	fatalIf(os.Setenv("PATH", bin))
	fatalIf(os.Setenv("AGENT_STARTED", started))
	client := llm.NewAgentClient("auto", "claude", nil)
	client.MaxRetries = 1
	handled := make(chan string, 1)
	server := httptest.NewServer(mcp.NewServer(func(ctx context.Context, _ string, _ map[string]any) (any, error) {
		_, _, callErr := client.Classify(ctx, "system", "user", llm.ClassifyOptions{})
		if callErr != nil {
			handled <- callErr.Error()
			return nil, callErr
		}
		handled <- ""
		return nil, nil
	}))
	defer server.Close()

	requestContext, cancel := context.WithCancel(context.Background())
	defer cancel()
	body := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1}}}`
	request, err := http.NewRequestWithContext(requestContext, http.MethodPost, server.URL, strings.NewReader(body))
	fatalIf(err)
	request.Header.Set("Content-Type", "application/json")
	clientDone := make(chan string, 1)
	go func() {
		response, err := http.DefaultClient.Do(request)
		if err != nil {
			clientDone <- err.Error()
			return
		}
		defer response.Body.Close()
		clientDone <- response.Status
	}()

	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, err := os.Stat(started); err == nil {
			break
		}
		if time.Now().After(deadline) {
			fatalIf(fmt.Errorf("fake host agent did not start"))
		}
		time.Sleep(5 * time.Millisecond)
	}
	cancel()

	var handlerError string
	select {
	case handlerError = <-handled:
	case <-time.After(5 * time.Second):
		fatalIf(fmt.Errorf("MCP handler did not observe request cancellation"))
	}
	clientError := <-clientDone
	clientError = strings.ReplaceAll(clientError, server.URL, "<server-url>")
	pidBytes, err := os.ReadFile(started)
	fatalIf(err)
	pid, err := strconv.Atoi(string(pidBytes))
	fatalIf(err)
	childExited := syscall.Kill(pid, 0) == syscall.ESRCH

	fixture := result{
		Schema:        "symeraseme.go-oracle.agent-cancel.v1",
		GoVersion:     runtime.Version(),
		SourcesSHA256: map[string]string{},
		HandlerError:  handlerError,
		ClientError:   clientError,
		ChildExited:   childExited,
	}
	for _, source := range []string{
		"go.mod",
		"cmd/symeraseme/main.go",
		"internal/mcp/server.go",
		"internal/mcp/contract_handler.go",
		"internal/llm/agent.go",
		"internal/llm/llm.go",
		"internal/llm/factory.go",
		"internal/triage/classifier.go",
		"internal/replies/service.go",
		"internal/eventstore/store.go",
		"rust-tests/parity/oracle/agent-cancel/main.go",
	} {
		contents, err := os.ReadFile(source)
		fatalIf(err)
		digest := sha256.Sum256(contents)
		fixture.SourcesSHA256[source] = hex.EncodeToString(digest[:])
	}
	encoded, err := json.MarshalIndent(fixture, "", "  ")
	fatalIf(err)
	fatalIf(os.MkdirAll(filepath.Dir(*out), 0o755))
	fatalIf(os.WriteFile(*out, append(encoded, '\n'), 0o644))
	fmt.Printf("recorded %s with %s -> %s\n", fixture.Schema, fixture.GoVersion, *out)
}

func fatalIf(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
