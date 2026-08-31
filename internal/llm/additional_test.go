package llm

import (
	"context"
	"errors"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func TestBaseClientDefaultsAndErrorHierarchy(t *testing.T) {
	client := NewBaseClient("model", 0, nil)
	if client.MaxRetries != 3 || client.CostTracker == nil {
		t.Fatalf("default client = %#v", client)
	}
	plain := &Error{msg: "plain"}
	wrapped := &Error{msg: "outer", err: plain}
	if wrapped.Error() != "outer: plain" || !errors.Is(wrapped, plain) || wrapped.Unwrap() != plain {
		t.Fatalf("error wrapping = %v", wrapped)
	}
	rate := &RateLimitError{Err: Error{msg: "rate"}}
	provider := &ProviderError{Err: Error{msg: "provider"}}
	if rate.Error() != "rate" || provider.Error() != "provider" || AsError(rate) == nil {
		t.Fatal("error hierarchy mismatch")
	}
}

func TestBaseClientStopsOnContextAndNonRetryableErrors(t *testing.T) {
	client := NewBaseClient("model", 3, nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, _, err := client.Classify(ctx, "system", "user", ClassifyOptions{}, func(context.Context, string, string, ClassifyOptions) (string, UsageRecord, error) {
		return "", UsageRecord{}, &RateLimitError{Err: Error{msg: "rate"}}
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled classify error = %v", err)
	}
	single := NewBaseClient("model", 1, nil)
	_, _, err = single.Classify(context.Background(), "", "", ClassifyOptions{}, func(context.Context, string, string, ClassifyOptions) (string, UsageRecord, error) {
		return "", UsageRecord{}, errors.New("not retryable")
	})
	if err == nil || !strings.Contains(err.Error(), "not retryable") {
		t.Fatalf("non-retryable error = %v", err)
	}
	if !isContextDone(ctx) || isContextDone(context.Background()) {
		t.Fatal("context detection mismatch")
	}
	if sleepCtx(ctx, time.Second) {
		t.Fatal("cancelled context sleep returned success")
	}
}

func TestLLMHelperErrorExtractionAndCacheKeys(t *testing.T) {
	input := "same"
	inputAgain := strings.Join([]string{"sa", "me"}, "")
	if hashCacheKey("") != 0 || hashCacheKey(input) != hashCacheKey(inputAgain) {
		t.Fatal("cache key hash is not stable")
	}
	var rate *RateLimitError
	if !asRateLimit(&RateLimitError{Err: Error{msg: "rate"}}, &rate) || rate == nil {
		t.Fatal("rate limit extraction failed")
	}
	var generic *Error
	if !asLLMError(&Error{msg: "generic"}, &generic) || generic == nil {
		t.Fatal("LLM error extraction failed")
	}
	cmd := exec.Command("/bin/sh", "-c", "exit 1")
	exitErr := cmd.Run()
	var processErr *exec.ExitError
	if !asExitError(exitErr, &processErr) || processErr == nil {
		t.Fatal("exit error extraction failed")
	}
	if llmkitID("openai-compatible") != "custom" || llmkitID("openai") != "openai" {
		t.Fatal("llmkit provider ID mapping failed")
	}
	if len(knownLLMKitProviders()) == 0 {
		t.Fatal("known provider list is empty")
	}
	if (&llmkitProvider{}).IsAvailable() {
		t.Fatal("nil llmkit provider reported available")
	}
	if (&llmkitProvider{}).Close() != nil {
		t.Fatal("llmkit provider close failed")
	}
}
