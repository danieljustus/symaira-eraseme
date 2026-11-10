// Command provider-cancel records Go MCP HTTP cancellation through llmkit.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

type result struct {
	Schema             string            `json:"schema"`
	GoVersion          string            `json:"go_version"`
	SourcesSHA256      map[string]string `json:"sources_sha256"`
	ProviderCanceled   bool              `json:"provider_canceled"`
	ClientDisconnected bool              `json:"client_disconnected"`
}

func main() {
	out := flag.String("fixture", "tests/fixtures/provider-cancel/http.json", "fixture output path")
	flag.Parse()

	providerCanceled := make(chan struct{})
	providerStarted := make(chan struct{})
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		close(providerStarted)
		<-r.Context().Done()
		close(providerCanceled)
	}))
	defer provider.Close()

	client, err := llm.NewLLMKitClient("openai", "oracle-model", provider.URL, "", "synthetic-key", nil)
	fatalIf(err)

	handlerCanceled := make(chan struct{})
	server := httptest.NewServer(mcp.NewServer(func(ctx context.Context, _ string, _ map[string]any) (any, error) {
		go func() {
			<-ctx.Done()
			close(handlerCanceled)
		}()
		_, _, callErr := client.Classify(ctx, "system", "user", llm.ClassifyOptions{})
		if callErr != nil {
			return nil, callErr
		}
		return nil, nil
	}))
	defer server.Close()

	body := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"classify_reply","arguments":{"request_id":1}}}`
	connection, err := net.Dial("tcp", server.Listener.Addr().String())
	fatalIf(err)
	_, err = fmt.Fprintf(connection, "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: %d\r\n\r\n%s", len(body), body)
	fatalIf(err)

	select {
	case <-providerStarted:
	case <-time.After(5 * time.Second):
		fatalIf(fmt.Errorf("MCP handler did not reach local provider"))
	}
	fatalIf(connection.Close())
	select {
	case <-handlerCanceled:
	case <-time.After(5 * time.Second):
		fatalIf(fmt.Errorf("go mcp request context did not observe client disconnect"))
	}
	select {
	case <-providerCanceled:
	case <-time.After(5 * time.Second):
		fatalIf(fmt.Errorf("llmkit provider request did not observe go mcp cancellation"))
	}

	fixture := result{
		Schema:             "symeraseme.go-oracle.provider-cancel.v1",
		GoVersion:          runtime.Version(),
		SourcesSHA256:      map[string]string{},
		ProviderCanceled:   true,
		ClientDisconnected: true,
	}
	for _, source := range []string{
		"go.mod",
		"go.sum",
		"cmd/symeraseme/main.go",
		"internal/mcp/server.go",
		"internal/mcp/contract_handler.go",
		"internal/llm/factory.go",
		"internal/llm/llm.go",
		"internal/llm/llmkit.go",
		"internal/triage/classifier.go",
		"internal/replies/service.go",
		"internal/eventstore/store.go",
		"rust-tests/parity/oracle/provider-cancel/main.go",
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
