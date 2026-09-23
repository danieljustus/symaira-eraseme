// Command llm-provider-surface is the byte oracle for the parts of internal/llm
// that stay inside this repository.
//
// It covers the provider descriptor table, the resolution order, the error
// taxonomy, the usage record and the retry loop's attempt accounting. The
// llmkit-backed transports, their credential resolution and their error texts
// live in corekit and have no Rust counterpart, so they are recorded as
// boundaries instead of cases.
//
// Every case runs with a frozen environment and frozen inputs; the fixture holds
// no timestamps and no durations, so two runs are byte-identical.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"sort"

	"github.com/danieljustus/symaira-eraseme/internal/llm"
)

const (
	fixturePath = "tests/fixtures/llm-provider-surface/cases.json"
	schema      = "symeraseme.go-oracle.llm-surface.v1"
)

// recordedUsage is the JSON shape of a usage record, key order fixed by the
// struct rather than by map iteration.
type recordedUsage struct {
	Model               string  `json:"model"`
	InputTokens         int     `json:"input_tokens"`
	OutputTokens        int     `json:"output_tokens"`
	CacheCreationTokens int     `json:"cache_creation_tokens"`
	CacheReadTokens     int     `json:"cache_read_tokens"`
	Cost                float64 `json:"cost"`
}

// createCase freezes one Create() call and what it produced.
type createCase struct {
	ID              string            `json:"id"`
	Env             map[string]string `json:"env"`
	Provider        string            `json:"provider"`
	Model           string            `json:"model"`
	AgentBackend    string            `json:"agent_backend"`
	Client          bool              `json:"client"`
	AgentModel      string            `json:"agent_model"`
	AgentMaxRetries int               `json:"agent_max_retries"`
	AgentTrackerLen int               `json:"agent_tracker_len"`
	AgentAvailable  bool              `json:"agent_available"`
	ErrorType       string            `json:"error_type"`
	ErrorPrefix     string            `json:"error_prefix"`
	ErrorNames      []string          `json:"error_names,omitempty"`
}

// usageCase freezes one usage record: the input the port must build, and the map
// the Go implementation renders for it.
type usageCase struct {
	Input  recordedUsage  `json:"input"`
	Record map[string]any `json:"record"`
}

// retryStep is one scripted answer of the injected API call.
type retryStep struct {
	Kind string `json:"kind"`
	Text string `json:"text"`
	Msg  string `json:"msg"`
}

// retryCase freezes one Classify() run over a scripted call.
type retryCase struct {
	ID               string        `json:"id"`
	MaxRetries       int           `json:"max_retries"`
	ZeroValue        bool          `json:"zero_value"`
	CacheKey         string        `json:"cache_key"`
	MaxTokens        int           `json:"max_tokens"`
	Temperature      float64       `json:"temperature"`
	Script           []retryStep   `json:"script"`
	Attempts         int           `json:"attempts"`
	AttemptsObserved bool          `json:"attempts_observed"`
	Text             string        `json:"text"`
	Usage            recordedUsage `json:"usage"`
	Error            *string       `json:"error,omitempty"`
	ErrorType        string        `json:"error_type,omitempty"`
}

type boundary struct {
	Path     string `json:"path"`
	Evidence string `json:"evidence"`
	Reason   string `json:"reason"`
}

type fixture struct {
	Schema         string            `json:"schema"`
	Source         string            `json:"source"`
	Providers      []string          `json:"providers"`
	UsageRecords   []usageCase       `json:"usage_records"`
	CacheKeyJitter map[string]int64  `json:"cache_key_jitter"`
	CreateCases    []createCase      `json:"create_cases"`
	RetryCases     []retryCase       `json:"retry_cases"`
	HostAgent      hostAgentProtocol `json:"host_agent_protocol"`
	Boundaries     []boundary        `json:"boundaries"`
}

func main() {
	out := flag.String("fixture", fixturePath, "where to write the recorded fixture")
	flag.Parse()

	recorded := fixture{
		Schema:         schema,
		Source:         "internal/llm/{llm,factory,util,agent}.go",
		CacheKeyJitter: map[string]int64{},
	}

	recorded.Providers = llm.ListAvailableProviders()
	sort.Strings(recorded.Providers)

	recorded.UsageRecords = recordUsageRecords()
	recorded.CacheKeyJitter = recordCacheKeyJitter()
	recorded.CreateCases = recordCreateCases()
	recorded.RetryCases = recordRetryCases()
	recorded.HostAgent = recordHostAgentProtocol()
	recorded.Boundaries = recordBoundaries()

	encoded, err := json.MarshalIndent(recorded, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "marshal fixture: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*out, append(encoded, '\n'), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "write fixture: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("recorded %d create, %d retry, %d usage, %d jitter cases -> %s\n",
		len(recorded.CreateCases), len(recorded.RetryCases),
		len(recorded.UsageRecords), len(recorded.CacheKeyJitter), *out)
	fmt.Printf("providers=%v\n", recorded.Providers)
}
