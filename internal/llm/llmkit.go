// LLMKitClient is the shared provider client over corekit/llmkit.
//
// It covers the anthropic, openai, ollama and openai-compatible providers
// without reimplementing any wire transport: the descriptor (endpoint,
// dialect, auth scheme, capabilities) comes from llmkit's embedded registry,
// the credential resolution (symvault://, env://, bare env name) is owned by
// llmkit, and provider failures are mapped onto llmkit's contract error
// taxonomy (llm_errors.json), which the Python port reimplemented as
// LLMClientError / LLMClientRateLimitError. This file keeps zero duplicate
// provider-client code (issue #721 definition of done).
package llm

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/danieljustus/symaira-corekit/llmkit"
)

// llmkitProvider is a provider backed by a llmkit descriptor.
type llmkitProvider struct {
	BaseClient
	client *llmkit.Client
	desc   llmkit.Descriptor
}

// llmkitID maps an EraseMe provider name to the llmkit registry descriptor
// id. The Python name "openai-compatible" is the llmkit "custom" descriptor
// (base URL required override, configurable dialect).
func llmkitID(provider string) string {
	if provider == "openai-compatible" {
		return "custom"
	}
	return provider
}

// NewLLMKitClient builds a client for a llmkit-backed provider.
//
// provider is an EraseMe provider name ("anthropic", "openai", "ollama",
// "openai-compatible"). baseURL overrides the descriptor endpoint (used by
// openai-compatible/custom endpoints and local Ollama). apiKey, when
// non-empty, is passed directly to llmkit (WithAPIKey) — for consumers that
// hold their own secret store. Otherwise credRef is resolved through
// llmkit's shared reference format (symvault://, env://NAME, bare env name).
func NewLLMKitClient(provider, model, baseURL, credRef, apiKey string, costTracker []UsageRecord) (*llmkitProvider, error) {
	desc, ok := llmkit.Lookup(llmkitID(provider))
	if !ok {
		known := knownLLMKitProviders()
		return nil, &ProviderError{Err: Error{msg: fmt.Sprintf(
			"unknown LLM provider %q. Known llmkit providers: %s (and \"agent\")", provider, strings.Join(known, ", "))}}
	}
	if model == "" {
		model = desc.DefaultModel()
	}

	opts := []llmkit.Option{llmkit.WithTimeout(defaultLLMTimeout)}
	if baseURL != "" {
		opts = append(opts, llmkit.WithBaseURL(baseURL))
	}
	if apiKey != "" {
		opts = append(opts, llmkit.WithAPIKey(apiKey))
	}
	cl, err := llmkit.NewClient(desc, credRef, opts...)
	if err != nil {
		return nil, &Error{msg: fmt.Sprintf("llmkit client for %q: %v", provider, err), err: err}
	}

	return &llmkitProvider{
		BaseClient: NewBaseClient(model, 3, costTracker),
		client:     cl,
		desc:       desc,
	}, nil
}

// IsAvailable reports whether the client can make API calls. For llmkit
// providers this is a light local check: the client exists and, for
// keyed providers, the credential resolved. We do not hit the network here
// (the Python is_available() checked SDK presence / key presence only).
func (p *llmkitProvider) IsAvailable() bool {
	return p.client != nil
}

// Classify sends a non-streaming chat completion (system + user prompt) and
// returns the first-choice text plus a usage record.
func (p *llmkitProvider) Classify(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions) (string, UsageRecord, error) {
	return p.BaseClient.Classify(ctx, systemPrompt, userPrompt, opts, p.callAPI)
}

func (p *llmkitProvider) callAPI(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions) (string, UsageRecord, error) {
	chatOpts := &llmkit.ChatOptions{
		System:    systemPrompt,
		MaxTokens: opts.MaxTokens,
	}
	if opts.Temperature != 0 {
		t := opts.Temperature
		chatOpts.Temperature = &t
	}

	choice, err := p.client.Chat(ctx, p.Model, []llmkit.Message{{Role: "user", Content: userPrompt}}, chatOpts)
	if err != nil {
		return "", UsageRecord{}, mapLLMKitError(err)
	}
	// llmkit's Chat returns the text only; usage is not surfaced on the
	// non-streaming path (streaming carries no usage either). The Python
	// client read usage from the SDK; the Go port records what llmkit
	// exposes — cost tracking degrades to 0 until llmkit surfaces usage.
	rec := UsageRecord{Model: p.Model}
	return strings.TrimSpace(choice.Content), rec, nil
}

func (p *llmkitProvider) Close() error { return nil }

// mapLLMKitError converts an llmkit error into the EraseMe error hierarchy,
// keeping the rate-limit distinction callers branch on.
func mapLLMKitError(err error) error {
	e := llmkit.AsError(err)
	if e == nil {
		return &Error{msg: err.Error(), err: err}
	}
	switch e.Code {
	case llmkit.ErrCodeRateLimited:
		return &RateLimitError{Err: Error{msg: e.Error(), err: e}}
	default:
		return &Error{msg: e.Error(), err: e}
	}
}

// defaultLLMTimeout is the per-request timeout for llmkit-backed providers.
const defaultLLMTimeout = 2 * time.Minute

// knownLLMKitProviders lists the llmkit registry ids relevant to EraseMe.
func knownLLMKitProviders() []string {
	var out []string
	for _, id := range []string{"anthropic", "openai", "ollama", "openai-compatible", "custom"} {
		if _, ok := llmkit.Lookup(id); ok {
			out = append(out, id)
		}
	}
	return out
}
