// Command llmkit-transport records the actual Go llmkit-backed non-streaming
// request contract using only local httptest servers and synthetic credentials.
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
	"strings"
	"sync/atomic"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

type observation struct {
	ID         string            `json:"id"`
	Provider   string            `json:"provider"`
	Model      string            `json:"model"`
	Path       string            `json:"path"`
	Headers    map[string]string `json:"headers"`
	Request    map[string]any    `json:"request"`
	Text       string            `json:"text"`
	UsageModel string            `json:"usage_model"`
}

type fixture struct {
	Schema   string               `json:"schema"`
	GoModule string               `json:"go_module"`
	Sources  map[string]string    `json:"sources_sha256"`
	Cases    []observation        `json:"cases"`
	Failures []failureObservation `json:"failure_cases"`
	Errors   []errorObservation   `json:"construction_errors"`
}

type errorObservation struct {
	ID       string `json:"id"`
	Provider string `json:"provider"`
	BaseURL  string `json:"base_url"`
	APIKey   string `json:"api_key"`
	Message  string `json:"message"`
}

type failureObservation struct {
	ID        string `json:"id"`
	Attempts  int    `json:"attempts"`
	ErrorKind string `json:"error_kind"`
	Message   string `json:"message"`
	Text      string `json:"text"`
}

func main() {
	out := flag.String("fixture", "tests/fixtures/llmkit-transport/cases.json", "fixture output path")
	flag.Parse()
	_, here, _, _ := runtime.Caller(0)
	root := filepath.Clean(filepath.Join(filepath.Dir(here), "../../../.."))
	f := fixture{Schema: "symeraseme.go-oracle.llmkit-transport.v1", GoModule: "github.com/danieljustus/symaira-corekit v0.16.2"}
	f.Sources = map[string]string{}
	for _, name := range []string{"internal/llm/factory.go", "internal/llm/llmkit.go", "internal/llm/llm.go", "go.mod", "go.sum"} {
		b, err := os.ReadFile(filepath.Join(root, name))
		fatalIf(err)
		sum := sha256.Sum256(b)
		f.Sources[name] = hex.EncodeToString(sum[:])
	}
	for _, spec := range []struct {
		id, provider, model string
		env                 map[string]string
		key, base           string
	}{
		{id: "openai-env-default-model", provider: "openai", env: map[string]string{"OPENAI_API_KEY": "synthetic-openai-key"}},
		{id: "openai-direct-key-wins", provider: "openai", key: "synthetic-direct-openai-key", env: map[string]string{"OPENAI_API_KEY": "ignored-environment-key"}},
		{id: "anthropic-env-explicit-model", provider: "anthropic", model: "claude-test", env: map[string]string{"ANTHROPIC_API_KEY": "synthetic-anthropic-key"}},
		{id: "ollama-host-no-auth", provider: "ollama", env: map[string]string{"OLLAMA_HOST_PLACEHOLDER": ""}},
		{id: "custom-direct-key", provider: "openai-compatible", model: "vendor-model", key: "synthetic-custom-key"},
	} {
		path, headers, request, text, usageModel := runCase(spec.id, spec.provider, spec.model, spec.env, spec.key, spec.base)
		f.Cases = append(f.Cases, observation{spec.id, spec.provider, spec.model, path, headers, request, text, usageModel})
	}
	f.Errors = recordConstructionErrors()
	f.Failures = recordFailures()
	b, err := json.MarshalIndent(f, "", "  ")
	fatalIf(err)
	fatalIf(os.MkdirAll(filepath.Dir(*out), 0o755))
	fatalIf(os.WriteFile(*out, append(b, '\n'), 0o644))
	fmt.Printf("recorded %d success, %d failure, and %d construction cases to %s\n", len(f.Cases), len(f.Failures), len(f.Errors), *out)
}

func recordFailures() []failureObservation {
	out := make([]failureObservation, 0, 3)
	for _, spec := range []struct {
		id     string
		status int
		body   string
	}{
		{"openai-rate-limit-exhausted", http.StatusTooManyRequests, `{"error":{"message":"try later","type":"rate_limit_error"}}`},
		{"openai-invalid-json-response", http.StatusOK, `{not json`},
		{"openai-empty-choices-response", http.StatusOK, `{"choices":[]}`},
	} {
		var attempts atomic.Int64
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			attempts.Add(1)
			w.WriteHeader(spec.status)
			_, _ = io.WriteString(w, spec.body)
		}))
		for _, name := range []string{"SYMERASEME_LLM_PROVIDER", "SYMERASEME_LLM_MODEL", "SYMERASEME_LLM_BASE_URL", "OLLAMA_HOST", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"} {
			fatalIf(os.Unsetenv(name))
		}
		fatalIf(os.Setenv("SYMERASEME_LLM_BASE_URL", srv.URL))
		client, err := llm.Create(llm.CreateOptions{Provider: "openai", APIKey: "synthetic-failure-key"})
		fatalIf(err)
		text, _, err := client.Classify(context.Background(), "system", "user", llm.ClassifyOptions{MaxTokens: 64, Temperature: 0.25})
		observation := failureObservation{ID: spec.id, Attempts: int(attempts.Load()), Text: text}
		if err == nil {
			observation.ErrorKind = "none"
		} else {
			observation.Message = err.Error()
			var rate *llm.RateLimitError
			var provider *llm.Error
			switch {
			case errors.As(err, &rate):
				observation.ErrorKind = "rate_limit"
			case errors.As(err, &provider):
				observation.ErrorKind = "provider"
			default:
				observation.ErrorKind = "other"
			}
		}
		srv.Close()
		fatalIf(os.Unsetenv("SYMERASEME_LLM_BASE_URL"))
		out = append(out, observation)
	}
	return out
}

func recordConstructionErrors() []errorObservation {
	for _, name := range []string{"SYMERASEME_LLM_PROVIDER", "SYMERASEME_LLM_MODEL", "SYMERASEME_LLM_BASE_URL", "OLLAMA_HOST", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"} {
		fatalIf(os.Unsetenv(name))
	}
	out := make([]errorObservation, 0, 2)
	for _, spec := range []struct{ id, provider, baseURL, apiKey string }{
		{id: "custom-base-url-before-credential", provider: "openai-compatible"},
		{id: "openai-missing-default-credential", provider: "openai", baseURL: "http://127.0.0.1:1234/v1"},
		{id: "openai-reject-insecure-remote-before-credential", provider: "openai", baseURL: "http://api.example.com/v1", apiKey: "synthetic-key"},
	} {
		_, err := llm.Create(llm.CreateOptions{Provider: spec.provider, BaseURL: spec.baseURL, APIKey: spec.apiKey})
		if err == nil {
			fatalIf(fmt.Errorf("%s: expected a constructor error", spec.id))
		}
		out = append(out, errorObservation{ID: spec.id, Provider: spec.provider, BaseURL: spec.baseURL, APIKey: spec.apiKey, Message: err.Error()})
	}
	return out
}

func runCase(id, provider, model string, env map[string]string, key, _ string) (string, map[string]string, map[string]any, string, string) {
	var gotPath string
	var gotHeaders map[string]string
	var gotRequest map[string]any
	response := `{"choices":[{"message":{"content":" hello ` + id + ` "}}]}`
	if provider == "anthropic" {
		response = `{"content":[{"type":"text","text":" hello ` + id + ` "}]}`
	}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotHeaders = map[string]string{
			"authorization":     strings.TrimSpace(r.Header.Get("Authorization")),
			"x-api-key":         strings.TrimSpace(r.Header.Get("x-api-key")),
			"anthropic-version": strings.TrimSpace(r.Header.Get("anthropic-version")),
		}
		body, err := io.ReadAll(r.Body)
		fatalIf(err)
		fatalIf(json.Unmarshal(body, &gotRequest))
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, response)
	}))
	defer srv.Close()
	for _, name := range []string{"SYMERASEME_LLM_PROVIDER", "SYMERASEME_LLM_MODEL", "SYMERASEME_LLM_BASE_URL", "OLLAMA_HOST", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"} {
		fatalIf(os.Unsetenv(name))
	}
	for name, value := range env {
		if name == "OLLAMA_HOST_PLACEHOLDER" {
			name = "OLLAMA_HOST"
			value = srv.URL
		}
		fatalIf(os.Setenv(name, value))
	}
	if provider == "ollama" {
		fatalIf(os.Setenv("OLLAMA_HOST", srv.URL))
	} else if provider == "openai" || provider == "anthropic" {
		fatalIf(os.Setenv("SYMERASEME_LLM_BASE_URL", srv.URL))
	}
	opts := llm.CreateOptions{Provider: provider, Model: model, APIKey: key}
	if provider == "openai-compatible" {
		opts.BaseURL = srv.URL
	}
	c, err := llm.Create(opts)
	fatalIf(err)
	text, usage, err := c.Classify(context.Background(), "system", "user", llm.ClassifyOptions{MaxTokens: 64, Temperature: 0.25})
	fatalIf(err)
	for name := range env {
		if name == "OLLAMA_HOST_PLACEHOLDER" {
			name = "OLLAMA_HOST"
		}
		fatalIf(os.Unsetenv(name))
	}
	fatalIf(os.Unsetenv("SYMERASEME_LLM_BASE_URL"))
	return gotPath, gotHeaders, gotRequest, text, usage.Model
}

func fatalIf(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
