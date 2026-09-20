package main

import (
	"context"
	"errors"
	"fmt"
	"hash/fnv"
	"os"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

// managedEnv is every variable the resolution order reads; each case starts from
// a cleared set so no case can inherit another one's environment.
var managedEnv = []string{
	"PATH",
	"SYMERASEME_LLM_PROVIDER",
	"SYMERASEME_LLM_MODEL",
	"SYMERASEME_LLM_BASE_URL",
	"SYMERASEME_AGENT_BACKEND",
	"ANTHROPIC_API_KEY",
	"OPENAI_API_KEY",
	"OLLAMA_HOST",
}

func recordUsageRecords() []usageCase {
	records := []llm.UsageRecord{
		{},
		{Model: "claude-sonnet-4-6", InputTokens: 1200, OutputTokens: 340, CacheCreationTokens: 0, CacheReadTokens: 512, Cost: 0.0123},
		{Model: "llama3.1", InputTokens: 1, OutputTokens: 0, CacheCreationTokens: 7, CacheReadTokens: 0, Cost: 0},
	}
	out := make([]usageCase, 0, len(records))
	for _, record := range records {
		out = append(out, usageCase{
			Input: recordedUsage{
				Model:               record.Model,
				InputTokens:         record.InputTokens,
				OutputTokens:        record.OutputTokens,
				CacheCreationTokens: record.CacheCreationTokens,
				CacheReadTokens:     record.CacheReadTokens,
				Cost:                record.Cost,
			},
			Record: record.Record(),
		})
	}
	return out
}

// recordCacheKeyJitter derives the retry jitter with Go's FNV-1a 32-bit hash,
// the same primitive the unexported hashCacheKey uses: `fnv32a(key) % 5`.
func recordCacheKeyJitter() map[string]int64 {
	keys := []string{
		"",
		"a",
		"abc",
		"seed-1",
		"triage-2026-09-20",
		"Grüße-ü",
		strings.Repeat("x", 64),
	}
	out := make(map[string]int64, len(keys))
	for _, key := range keys {
		// hashCacheKey short-circuits an empty key to 0 without hashing.
		if key == "" {
			out[key] = 0
			continue
		}
		out[key] = int64(fnv32a(key) % 5)
	}
	return out
}

func fnv32a(key string) uint32 {
	hash := fnv.New32a()
	if _, err := hash.Write([]byte(key)); err != nil {
		panic(err)
	}
	return hash.Sum32()
}

func recordCreateCases() []createCase {
	type request struct {
		id           string
		env          map[string]string
		provider     string
		model        string
		agentBackend string
	}
	requests := []request{
		{
			id:       "agent-default",
			env:      map[string]string{"PATH": ""},
			provider: "agent",
		},
		{
			id:       "agent-explicit-model",
			env:      map[string]string{"PATH": ""},
			provider: "agent",
			model:    "model-x",
		},
		{
			id:       "agent-model-from-environment",
			env:      map[string]string{"PATH": "", "SYMERASEME_LLM_MODEL": "env-model"},
			provider: "agent",
		},
		{
			id:  "agent-provider-normalized",
			env: map[string]string{"PATH": "", "SYMERASEME_LLM_PROVIDER": "  AGENT  "},
		},
		{
			id:       "agent-model-keeps-whitespace",
			env:      map[string]string{"PATH": "", "SYMERASEME_LLM_MODEL": "  "},
			provider: "agent",
		},
		{
			id:           "agent-backend-from-environment",
			env:          map[string]string{"PATH": "", "SYMERASEME_AGENT_BACKEND": "codex"},
			provider:     "agent",
			agentBackend: "",
		},
		{
			id:       "unknown-provider",
			env:      map[string]string{"PATH": ""},
			provider: "gemini",
		},
		{
			id:       "unknown-provider-quotes-escaped",
			env:      map[string]string{"PATH": ""},
			provider: "gem\tini\n\"x\"",
		},
	}

	out := make([]createCase, 0, len(requests))
	for _, request := range requests {
		clearManagedEnv()
		for name, value := range request.env {
			if err := os.Setenv(name, value); err != nil {
				panic(err)
			}
		}

		recorded := createCase{
			ID:           request.id,
			Env:          request.env,
			Provider:     request.provider,
			Model:        request.model,
			AgentBackend: request.agentBackend,
		}

		client, err := llm.Create(llm.CreateOptions{
			Provider:     request.provider,
			Model:        request.model,
			AgentBackend: request.agentBackend,
		})
		switch {
		case err != nil:
			recorded.ErrorType = errorType(err)
			prefix, names := splitProviderError(err.Error())
			recorded.ErrorPrefix = prefix
			recorded.ErrorNames = names
		default:
			recorded.Client = client != nil
			if agent, ok := client.(*llm.AgentClient); ok {
				recorded.AgentModel = agent.Model
				recorded.AgentMaxRetries = agent.MaxRetries
				recorded.AgentTrackerLen = len(agent.CostTracker)
				recorded.AgentAvailable = agent.IsAvailable()
			}
		}

		out = append(out, recorded)
	}
	clearManagedEnv()
	return out
}

// splitProviderError separates the fixed message prefix from the provider list,
// whose order comes from Go's map iteration and is therefore not reproducible.
func splitProviderError(message string) (string, []string) {
	const marker = "Known providers: "
	index := strings.Index(message, marker)
	if index < 0 {
		return message, nil
	}
	prefix := message[:index+len(marker)]
	names := strings.Split(message[index+len(marker):], ", ")
	sort.Strings(names)
	return prefix, names
}

func errorType(err error) string {
	var provider *llm.ProviderError
	var rateLimit *llm.RateLimitError
	var generic *llm.Error
	switch {
	case errors.As(err, &provider):
		return "ProviderError"
	case errors.As(err, &rateLimit):
		return "RateLimitError"
	case errors.As(err, &generic):
		return "Error"
	default:
		return fmt.Sprintf("%T", err)
	}
}

func recordRetryCases() []retryCase {
	script := func(steps ...retryStep) []retryStep { return steps }

	requests := []retryCase{
		{
			ID:         "success-first-attempt",
			MaxRetries: 3,
			Script:     script(retryStep{Kind: "ok", Text: "triage: remove"}),
		},
		{
			ID:         "rate-limit-then-success",
			MaxRetries: 3,
			Script:     script(retryStep{Kind: "rate_limit", Msg: "429 too many requests"}, retryStep{Kind: "ok", Text: "second try"}),
		},
		{
			ID:         "foreign-error-is-not-retried",
			MaxRetries: 2,
			Script:     script(retryStep{Kind: "error", Msg: "boom"}, retryStep{Kind: "ok", Text: "never reached"}),
		},
		{
			ID:         "rate-limit-exhausted",
			MaxRetries: 2,
			Script:     script(retryStep{Kind: "rate_limit", Msg: "rate limited"}, retryStep{Kind: "rate_limit", Msg: "rate limited"}),
		},
		{
			ID:         "cancelled-context-wins-over-retry",
			MaxRetries: 3,
			Script:     script(retryStep{Kind: "error", Msg: "boom"}),
			// The cancellation is applied before the call, so the first answer is
			// still consumed and only then does the context win.
		},
		{
			ID:         "context-error-from-call-is-not-retried",
			MaxRetries: 3,
			Script:     script(retryStep{Kind: "ctx_error", Msg: "context canceled"}, retryStep{Kind: "ok", Text: "never reached"}),
		},
		{
			ID:         "rate-limit-jitter-key",
			MaxRetries: 3,
			CacheKey:   "seed-1",
			Script:     script(retryStep{Kind: "rate_limit", Msg: "429"}, retryStep{Kind: "ok", Text: "after jitter"}),
		},
	}

	out := make([]retryCase, 0, len(requests)+1)
	for _, request := range requests {
		recorded := request
		recorded.MaxTokens = 64
		recorded.Temperature = 0.2
		recorded.Error = nil
		recorded.ErrorType = ""
		recorded.Text = ""
		recorded.Usage = recordedUsage{}

		base := llm.NewBaseClient("oracle-model", request.MaxRetries, nil)
		ctx, cancel := context.WithCancel(context.Background())
		if request.ID == "cancelled-context-wins-over-retry" {
			cancel()
		}

		attempts := 0
		text, usage, err := base.Classify(ctx, "system", "user", llm.ClassifyOptions{
			MaxTokens:   64,
			Temperature: 0.2,
			CacheKey:    request.CacheKey,
		}, func(_ context.Context, _, _ string, _ llm.ClassifyOptions) (string, llm.UsageRecord, error) {
			step := request.Script[min(attempts, len(request.Script)-1)]
			attempts++
			switch step.Kind {
			case "ok":
				return step.Text, llm.UsageRecord{Model: "oracle-model", InputTokens: 10, OutputTokens: 2, Cost: 0.001}, nil
			case "rate_limit":
				// Error carries no exported constructor, so the recorded rate-limit
				// failure uses the zero Error: the branch is what is pinned here.
				return "", llm.UsageRecord{}, &llm.RateLimitError{Err: llm.Error{}}
			case "ctx_error":
				return "", llm.UsageRecord{}, context.Canceled
			default:
				return "", llm.UsageRecord{}, errors.New(step.Msg)
			}
		})
		cancel()

		recorded.Attempts = attempts
		recorded.AttemptsObserved = true
		recorded.Text = text
		recorded.Usage = recordedUsage{
			Model:               usage.Model,
			InputTokens:         usage.InputTokens,
			OutputTokens:        usage.OutputTokens,
			CacheCreationTokens: usage.CacheCreationTokens,
			CacheReadTokens:     usage.CacheReadTokens,
			Cost:                usage.Cost,
		}
		if err != nil {
			message := err.Error()
			recorded.Error = &message
			recorded.ErrorType = errorType(err)
		}
		out = append(out, recorded)
	}

	// The retry loop returns its exhaustion message only when the loop body never
	// runs, which needs the zero value rather than NewBaseClient.
	zero := llm.BaseClient{Model: "oracle-model"}
	attempts := 0
	_, _, err := zero.Classify(context.Background(), "system", "user", llm.ClassifyOptions{},
		func(_ context.Context, _, _ string, _ llm.ClassifyOptions) (string, llm.UsageRecord, error) {
			attempts++
			return "", llm.UsageRecord{}, nil
		})
	zeroCase := retryCase{
		ID:               "zero-max-retries-never-calls",
		MaxRetries:       0,
		ZeroValue:        true,
		Attempts:         attempts,
		AttemptsObserved: true,
	}
	if err != nil {
		message := err.Error()
		zeroCase.Error = &message
		zeroCase.ErrorType = errorType(err)
	}
	out = append(out, zeroCase)

	// The host-agent client is the one provider whose callAPI lives in this
	// repository: with no CLI on PATH it returns a retryable *Error, so its text
	// is recorded here. The attempt count is not observable on that path.
	clearManagedEnv()
	if err := os.Setenv("PATH", ""); err != nil {
		panic(err)
	}
	agent, agentErr := llm.Create(llm.CreateOptions{Provider: "agent", Model: "auto"})
	if agentErr != nil {
		panic(agentErr)
	}
	_, _, agentCallErr := agent.Classify(context.Background(), "system", "user", llm.ClassifyOptions{})
	agentCase := retryCase{ID: "host-agent-unavailable", MaxRetries: 3}
	if agentCallErr != nil {
		message := agentCallErr.Error()
		agentCase.Error = &message
		agentCase.ErrorType = errorType(agentCallErr)
	}
	out = append(out, agentCase)
	clearManagedEnv()

	return out
}

// recordBoundaries captures what the public API shows for the llmkit-backed
// construction path, so the port's boundary carries measured evidence.
func recordBoundaries() []boundary {
	clearManagedEnv()
	if err := os.Setenv("PATH", ""); err != nil {
		panic(err)
	}
	_, err := llm.Create(llm.CreateOptions{})
	return []boundary{
		{
			Path:     "llmkit-backed construction (anthropic, openai, ollama, openai-compatible)",
			Evidence: fmt.Sprintf("Create(CreateOptions{}) -> %v", err),
			Reason:   "corekit/llmkit owns the transports, the credential reference format and this error text; it has no Rust counterpart, so construction stays Go.",
		},
		{
			Path:     "llmkit provider descriptor defaults (env key, default model, base URL)",
			Evidence: "not observable: the table is unexported and an llmkit client exposes no model accessor",
			Reason:   "the port pins the provider name set and the agent branch, which are observable; the llmkit defaults cannot be read back through the public API.",
		},
		{
			Path:     "cache key jitter for the empty key",
			Evidence: "hashCacheKey(\"\") short-circuits to 0 without hashing; the recorded value for the empty key is that 0, not fnv32a(\"\")%5",
			Reason:   "the empty key is the only input that skips the hash, so the fixture pins the rule rather than the primitive for it.",
		},
		{
			Path:     "retry backoff durations",
			Evidence: "time.Duration(1<<attempt)*time.Second plus fnv32a(cacheKey)%5 seconds; only the attempt accounting is recorded",
			Reason:   "wall-clock durations are not reproducible in a byte fixture; the jitter input is pinned separately as cache_key_jitter.",
		},
	}
}

func clearManagedEnv() {
	for _, name := range managedEnv {
		if err := os.Unsetenv(name); err != nil {
			panic(err)
		}
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
