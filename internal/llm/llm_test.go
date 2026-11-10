package llm

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestListAvailableProviders(t *testing.T) {
	got := ListAvailableProviders()
	want := map[string]bool{"anthropic": true, "openai": true, "ollama": true, "openai-compatible": true, "agent": true}
	seen := map[string]bool{}
	for _, p := range got {
		seen[p] = true
		if !want[p] {
			t.Errorf("unexpected provider %q", p)
		}
	}
	for name := range want {
		if !seen[name] {
			t.Errorf("missing provider %q in %v", name, got)
		}
	}
}

func TestCreateUnknownProvider(t *testing.T) {
	_, err := Create(CreateOptions{Provider: "definitely-not-a-provider"})
	var pe *ProviderError
	if !errors.As(err, &pe) {
		t.Fatalf("want *ProviderError, got %T: %v", err, err)
	}
	if !strings.Contains(pe.Error(), "unknown LLM provider") {
		t.Errorf("error = %q, want 'unknown LLM provider'", pe.Error())
	}
}

func TestCreateProviderEnvFallback(t *testing.T) {
	orig := envLookup
	envLookup = func(k string) string {
		if k == "SYMERASEME_LLM_PROVIDER" {
			return "agent"
		}
		return ""
	}
	defer func() { envLookup = orig }()

	c, err := Create(CreateOptions{})
	if err != nil {
		t.Fatalf("Create with env provider: %v", err)
	}
	if _, ok := c.(*AgentClient); !ok {
		t.Fatalf("want *AgentClient, got %T", c)
	}
}

func TestCreateOpenAICompatibleRequiresBaseURL(t *testing.T) {
	// llmkit's "custom" descriptor requires a base URL override; without it
	// the client construction must fail with a clear error.
	_, err := NewLLMKitClient("openai-compatible", "", "", "", "", nil)
	if err == nil {
		t.Fatal("openai-compatible without base URL should fail (custom descriptor requires override)")
	}
	if !strings.Contains(err.Error(), "base URL") {
		t.Errorf("error = %q, want mention of base URL", err.Error())
	}
}

func TestUsageRecordRecordShape(t *testing.T) {
	rec := UsageRecord{Model: "claude-sonnet-4-6", InputTokens: 10, OutputTokens: 5, CacheCreationTokens: 2, CacheReadTokens: 1, Cost: 0.25}
	m := rec.Record()
	for _, key := range []string{"model", "input_tokens", "output_tokens", "cache_creation_tokens", "cache_read_tokens", "cost"} {
		if _, ok := m[key]; !ok {
			t.Errorf("Record() missing key %q", key)
		}
	}
	if m["model"] != "claude-sonnet-4-6" || m["input_tokens"] != 10 || m["cost"] != 0.25 {
		t.Errorf("Record() = %v", m)
	}
}

func TestMapLLMKitErrorRateLimit(t *testing.T) {
	// llmkit maps HTTP 429 → ErrCodeRateLimited; our map must surface a
	// *RateLimitError so callers can retry.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusTooManyRequests)
		_, _ = w.Write([]byte(`{"error":{"message":"rate limit exceeded"}}`))
	}))
	defer srv.Close()

	c, err := NewLLMKitClient("openai", "gpt-4o", srv.URL, "", "test-key", nil)
	if err != nil {
		t.Fatalf("NewLLMKitClient: %v", err)
	}
	_, _, err = c.Classify(context.Background(), "sys", "user", ClassifyOptions{MaxTokens: 64})
	if err == nil {
		t.Fatal("expected error from 429")
	}
	var rl *RateLimitError
	if !errors.As(err, &rl) {
		t.Fatalf("want *RateLimitError, got %T: %v", err, err)
	}
}

func TestLLMKitClientClassifyHappyPath(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/chat/completions" {
			t.Errorf("path = %q, want /chat/completions", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"choices":[{"message":{"content":" hello world "},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3}}`))
	}))
	defer srv.Close()

	c, err := NewLLMKitClient("openai", "gpt-4o", srv.URL, "", "test-key", nil)
	if err != nil {
		t.Fatalf("NewLLMKitClient: %v", err)
	}
	text, rec, err := c.Classify(context.Background(), "sys", "user", ClassifyOptions{MaxTokens: 64})
	if err != nil {
		t.Fatalf("Classify: %v", err)
	}
	if text != "hello world" {
		t.Errorf("text = %q, want \"hello world\"", text)
	}
	if rec.Model != "gpt-4o" {
		t.Errorf("rec.Model = %q, want gpt-4o", rec.Model)
	}
}

func TestAgentCombinePrompts(t *testing.T) {
	got := combinePrompts("system part", "user part")
	want := "system part\n\n---\n\nuser part"
	if got != want {
		t.Errorf("combinePrompts = %q, want %q", got, want)
	}
}

func TestAgentClientNotAvailableWithoutCLI(t *testing.T) {
	// Stub cliOnPath to report nothing available → IsAvailable must be false.
	orig := cliOnPath
	cliOnPath = func(string) bool { return false }
	defer func() { cliOnPath = orig }()

	c := NewAgentClient("", "", nil)
	if c.IsAvailable() {
		t.Fatal("IsAvailable should be false with no agent CLI on PATH")
	}
	_, _, err := c.Classify(context.Background(), "s", "u", ClassifyOptions{})
	var e *Error
	if !errors.As(err, &e) {
		t.Fatalf("want *Error, got %T: %v", err, err)
	}
}

func TestAgentClientDetectBackendPreference(t *testing.T) {
	orig := cliOnPath
	cliOnPath = func(name string) bool {
		// Only hermes and copilot exist; claude should be skipped.
		return name != "claude"
	}
	defer func() { cliOnPath = orig }()

	c := NewAgentClient("", "", nil)
	got := c.detectBackend("")
	if got != "hermes" {
		t.Errorf("detectBackend = %q, want hermes (first available after claude)", got)
	}
}

func TestAgentClientExplicitBackendUnavailable(t *testing.T) {
	orig := cliOnPath
	cliOnPath = func(string) bool { return false }
	defer func() { cliOnPath = orig }()

	c := NewAgentClient("", "claude", nil)
	if got := c.detectBackend("claude"); got != "" {
		t.Errorf("detectBackend(claude) = %q, want \"\" (not on PATH)", got)
	}
}

func TestAsErrorUnwrap(t *testing.T) {
	base := &Error{msg: "inner"}
	wrapped := &ProviderError{Err: *base}
	if AsError(wrapped) == nil {
		t.Fatal("AsError should find *Error through ProviderError")
	}
}
