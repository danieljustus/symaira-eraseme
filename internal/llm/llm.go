// Package llm provides the Go-port LLM client layer for Symaira EraseMe.
//
// Port of src/symeraseme/llm/ from the Python implementation (issue #721).
// Provider transports are NOT reimplemented here: anthropic, openai, ollama
// and openai-compatible all go through corekit/llmkit, which owns the wire
// dialects, the credential reference format and the contract error taxonomy.
// The only EraseMe-specific backend is the host-agent provider ("agent"),
// which has no llmkit equivalent — it delegates to a coding-agent CLI.
package llm

import (
	"context"
	"errors"
	"fmt"
	"os"
	"time"
)

// UsageRecord is a single LLM usage and cost record, mirroring the Python
// protocol.UsageRecord exactly (same fields, same Record() shape) so the
// Go port keeps the same report surface.
type UsageRecord struct {
	Model               string
	InputTokens         int
	OutputTokens        int
	CacheCreationTokens int
	CacheReadTokens     int
	Cost                float64
}

// Record returns the record as a map, matching the Python record() output.
func (u UsageRecord) Record() map[string]any {
	return map[string]any{
		"model":                 u.Model,
		"input_tokens":          u.InputTokens,
		"output_tokens":         u.OutputTokens,
		"cache_creation_tokens": u.CacheCreationTokens,
		"cache_read_tokens":     u.CacheReadTokens,
		"cost":                  u.Cost,
	}
}

// Error is a generic error raised by any LLM provider client. Mirrors
// python LLMClientError.
type Error struct {
	msg string
	err error
}

func (e *Error) Error() string {
	if e.err != nil {
		return fmt.Sprintf("%s: %v", e.msg, e.err)
	}
	return e.msg
}

func (e *Error) Unwrap() error { return e.err }

// RateLimitError is a rate-limit failure — callers can inspect and retry.
// Mirrors python LLMClientRateLimitError.
type RateLimitError struct {
	Err Error
}

func (e *RateLimitError) Error() string { return e.Err.Error() }
func (e *RateLimitError) Unwrap() error { return &e.Err }

// ProviderError is raised when an unknown or unavailable provider is
// requested. Mirrors python LLMProviderError.
type ProviderError struct {
	Err Error
}

func (e *ProviderError) Error() string { return e.Err.Error() }
func (e *ProviderError) Unwrap() error { return &e.Err }

// ClassifyOptions carries per-call parameters, mirroring the Python
// classify() signature.
type ClassifyOptions struct {
	MaxTokens   int
	Temperature float64
	CacheKey    string
}

// Client is the Go-port equivalent of the Python LLMClient protocol. Any
// implementation with these methods is a valid Client.
type Client interface {
	// IsAvailable reports whether the client can make API calls right now.
	IsAvailable() bool
	// Classify sends a classification request and returns (response text,
	// usage record). Raises *Error on any provider failure and
	// *RateLimitError when the provider rate-limits the call.
	Classify(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions) (string, UsageRecord, error)
	// Close releases resources (connections, sessions). Optional for
	// providers that keep none.
	Close() error
}

// BaseClient provides the shared retry loop with exponential backoff that
// the Python BaseLLMClient implements. Embed it and provide IsAvailable and
// callAPI.
type BaseClient struct {
	Model       string
	MaxRetries  int
	CostTracker []UsageRecord
}

// NewBaseClient builds a BaseClient with the Python defaults (3 retries).
func NewBaseClient(model string, maxRetries int, costTracker []UsageRecord) BaseClient {
	if maxRetries <= 0 {
		maxRetries = 3
	}
	if costTracker == nil {
		costTracker = []UsageRecord{}
	}
	return BaseClient{Model: model, MaxRetries: maxRetries, CostTracker: costTracker}
}

// callAPI is implemented by each provider-specific client. It must raise
// *RateLimitError for rate-limit failures and *Error for other API errors.
type callAPI func(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions) (string, UsageRecord, error)

// Classify runs the unified retry loop (exponential backoff) around callAPI.
func (b *BaseClient) Classify(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions, fn callAPI) (string, UsageRecord, error) {
	var lastErr error
	for attempt := 1; attempt <= b.MaxRetries; attempt++ {
		text, usage, err := fn(ctx, systemPrompt, userPrompt, opts)
		if err == nil {
			return text, usage, nil
		}
		lastErr = err

		var rl *RateLimitError
		var ge *Error
		switch {
		case isContextDone(ctx):
			return "", UsageRecord{}, ctx.Err()
		case asRateLimit(err, &rl):
			if attempt < b.MaxRetries {
				wait := time.Duration(1<<attempt)*time.Second + time.Duration(hashCacheKey(opts.CacheKey)%5)*time.Second
				if !sleepCtx(ctx, wait) {
					return "", UsageRecord{}, ctx.Err()
				}
				continue
			}
			return "", UsageRecord{}, err
		case asLLMError(err, &ge):
			if attempt < b.MaxRetries {
				wait := time.Duration(1<<attempt) * time.Second
				if !sleepCtx(ctx, wait) {
					return "", UsageRecord{}, ctx.Err()
				}
				continue
			}
			return "", UsageRecord{}, err
		default:
			// Non-retryable (e.g. a context error that is not ours) — surface.
			return "", UsageRecord{}, err
		}
	}
	return "", UsageRecord{}, fmt.Errorf("all %d retries exhausted: %w", b.MaxRetries, lastErr)
}

// AsError extracts a *Error from err (or nil), walking Unwrap chains.
func AsError(err error) *Error {
	var e *Error
	if errors.As(err, &e) {
		return e
	}
	return nil
}

// envLookup is an indirection over os.Getenv so tests can inject values.
var envLookup = os.Getenv
