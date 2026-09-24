// Command llm-failures-next records provider-failure cases from the real
// Go llmkit transport against a local-only HTTP server.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"sync/atomic"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

type observation struct {
	Schema        string            `json:"schema"`
	GoModule      string            `json:"go_module"`
	SourcesSHA256 map[string]string `json:"sources_sha256"`
	ID            string            `json:"id"`
	Path          string            `json:"path"`
	Status        int               `json:"status"`
	Attempts      int               `json:"attempts"`
	ErrorKind     string            `json:"error_kind"`
	Message       string            `json:"message"`
	Text          string            `json:"text"`
	APIKey        string            `json:"api_key"`
	Body          string            `json:"body"`
}

func main() {
	status := flag.Int("status", http.StatusForbidden, "synthetic provider HTTP status (403 or 404)")
	flag.Parse()
	var id, apiKey, body string
	switch *status {
	case http.StatusForbidden:
		id, apiKey, body = "openai-forbidden-echoed-key", "synthetic-403-key", `permission denied; key synthetic-403-key`
	case http.StatusNotFound:
		id, apiKey, body = "openai-model-not-found-echoed-key", "synthetic-404-key", `model not found; key synthetic-404-key`
	case http.StatusInternalServerError:
		id, apiKey, body = "openai-provider-error-echoed-key", "synthetic-500-key", `upstream temporarily unavailable; key synthetic-500-key`
	default:
		fatalIf(fmt.Errorf("unsupported synthetic status %d", *status))
	}
	_, here, _, _ := runtime.Caller(0)
	root := filepath.Clean(filepath.Join(filepath.Dir(here), "../../../.."))
	obs := observation{
		Schema:   "symeraseme.go-oracle.llm-failures-next.v1",
		GoModule: "github.com/danieljustus/symaira-corekit v0.16.2",
		ID:       id,
		Status:   *status,
		APIKey:   apiKey,
		Body:     body,
	}
	obs.SourcesSHA256 = make(map[string]string)
	for _, name := range []string{"internal/llm/factory.go", "internal/llm/llmkit.go", "internal/llm/llm.go", "go.mod", "go.sum"} {
		b, err := os.ReadFile(filepath.Join(root, name))
		fatalIf(err)
		sum := sha256.Sum256(b)
		obs.SourcesSHA256[name] = hex.EncodeToString(sum[:])
	}

	var attempts atomic.Int64
	var requestPath atomic.Value
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		attempts.Add(1)
		requestPath.Store(r.URL.Path)
		w.WriteHeader(*status)
		_, _ = io.WriteString(w, body)
	}))
	defer srv.Close()
	for _, name := range []string{"SYMERASEME_LLM_PROVIDER", "SYMERASEME_LLM_MODEL", "SYMERASEME_LLM_BASE_URL", "OLLAMA_HOST", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"} {
		fatalIf(os.Unsetenv(name))
	}
	fatalIf(os.Setenv("SYMERASEME_LLM_BASE_URL", srv.URL))
	client, err := llm.Create(llm.CreateOptions{Provider: "openai", APIKey: apiKey})
	fatalIf(err)
	text, _, err := client.Classify(context.Background(), "system", "user", llm.ClassifyOptions{MaxTokens: 64, Temperature: 0.25})
	obs.Attempts = int(attempts.Load())
	obs.Path = requestPath.Load().(string)
	obs.Text = text
	if err == nil {
		obs.ErrorKind = "none"
	} else {
		obs.Message = err.Error()
		var rateLimit *llm.RateLimitError
		var provider *llm.Error
		switch {
		case errors.As(err, &rateLimit):
			obs.ErrorKind = "rate_limit"
		case errors.As(err, &provider):
			obs.ErrorKind = "provider"
		default:
			obs.ErrorKind = fmt.Sprintf("%T", err)
		}
	}

	var dst io.Writer = os.Stdout
	if path := os.Getenv("LLM_FAILURES_NEXT_FIXTURE"); path != "" {
		file, err := os.Create(path)
		fatalIf(err)
		defer file.Close()
		dst = file
	}
	enc := json.NewEncoder(dst)
	enc.SetIndent("", "  ")
	fatalIf(enc.Encode(obs))
}

func fatalIf(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
