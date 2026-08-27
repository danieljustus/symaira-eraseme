// Factory mirrors the Python llm/factory.py: provider registry, env-var
// fallbacks and the create_llm_client entry point. Provider resolution order
// and env names match the Python implementation exactly so existing
// SYMERASEME_LLM_* configuration keeps working unchanged.
package llm

import (
	"fmt"
	"strings"
)

const (
	envProvider = "SYMERASEME_LLM_PROVIDER"
	envModel    = "SYMERASEME_LLM_MODEL"
	envBaseURL  = "SYMERASEME_LLM_BASE_URL"
	envOllama   = "OLLAMA_HOST"
)

// providerSpec carries the per-provider construction defaults, mirroring
// Python _PROVIDERS (module/class/env/default) without the module/class
// indirection — llmkit owns those.
type providerSpec struct {
	// kind is "llmkit" or "agent".
	kind         string
	envKey       string
	defaultModel string
}

// providers maps provider name → construction spec.
var providers = map[string]providerSpec{
	"anthropic":         {kind: "llmkit", envKey: "ANTHROPIC_API_KEY", defaultModel: "claude-sonnet-4-6"},
	"openai":            {kind: "llmkit", envKey: "OPENAI_API_KEY", defaultModel: "gpt-4o"},
	"ollama":            {kind: "llmkit", envKey: "", defaultModel: "llama3.1"},
	"openai-compatible": {kind: "llmkit", envKey: "", defaultModel: "default"},
	"agent":             {kind: "agent", envKey: "", defaultModel: "auto"},
}

// ListAvailableProviders returns all known provider names.
func ListAvailableProviders() []string {
	out := make([]string, 0, len(providers))
	for name := range providers {
		out = append(out, name)
	}
	return out
}

// CreateOptions mirrors the Python create_llm_client parameters.
type CreateOptions struct {
	Provider     string
	Model        string
	APIKey       string
	BaseURL      string
	AgentBackend string
	CostTracker  []UsageRecord
}

// Create builds an LLM client for the selected provider, applying the same
// fallbacks as Python: provider ← SYMERASEME_LLM_PROVIDER ← "anthropic";
// model ← SYMERASEME_LLM_MODEL ← provider default; baseURL ←
// SYMERASEME_LLM_BASE_URL (openai-compatible/custom endpoints); ollama host
// ← OLLAMA_HOST ← localhost default.
func Create(opts CreateOptions) (Client, error) {
	provider := opts.Provider
	if provider == "" {
		provider = envLookup(envProvider)
	}
	if provider == "" {
		provider = "anthropic"
	}
	provider = strings.ToLower(strings.TrimSpace(provider))

	spec, ok := providers[provider]
	if !ok {
		return nil, &ProviderError{Err: Error{msg: fmt.Sprintf(
			"unknown LLM provider %q. Known providers: %s", provider, strings.Join(knownProviderNames(), ", "))}}
	}

	model := opts.Model
	if model == "" {
		if m := envLookup(envModel); m != "" {
			model = m
		} else {
			model = spec.defaultModel
		}
	}

	baseURL := opts.BaseURL
	if baseURL == "" {
		baseURL = envLookup(envBaseURL)
	}
	if spec.kind == "llmkit" && provider == "ollama" && baseURL == "" {
		// Honor OLLAMA_HOST like the Python factory did. llmkit speaks the
		// OpenAI-compatible surface on the /v1 path, so append /v1 to a
		// bare host. When unset, leave baseURL empty and let the llmkit
		// descriptor default (localhost:11434/v1) apply.
		if host := envLookup(envOllama); host != "" {
			baseURL = strings.TrimRight(host, "/") + "/v1"
		}
	}

	if spec.kind == "agent" {
		return NewAgentClient(model, opts.AgentBackend, opts.CostTracker), nil
	}

	// llmkit-backed. Explicit key wins over env resolution; otherwise let
	// llmkit resolve the provider's credential (bare env name / symvault://).
	client, err := NewLLMKitClient(provider, model, baseURL, spec.envKey, opts.APIKey, opts.CostTracker)
	if err != nil {
		return nil, err
	}
	return client, nil
}

func knownProviderNames() []string {
	out := make([]string, 0, len(providers))
	for name := range providers {
		out = append(out, name)
	}
	return out
}
