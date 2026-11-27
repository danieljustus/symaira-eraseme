package llm

import (
	"context"
	"errors"
	"hash/fnv"
	"os/exec"
	"time"
)

// isContextDone reports whether ctx was cancelled/expired.
func isContextDone(ctx context.Context) bool {
	select {
	case <-ctx.Done():
		return true
	default:
		return false
	}
}

// sleepCtx sleeps for d, aborting early when ctx is done. Returns false when
// aborted.
func sleepCtx(ctx context.Context, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}

// hashCacheKey derives a small stable jitter from the cache key, mirroring
// the Python `hash(str(cache_key)) % 5` retry jitter.
func hashCacheKey(key string) int64 {
	if key == "" {
		return 0
	}
	h := fnv.New32a()
	_, _ = h.Write([]byte(key))
	return int64(h.Sum32() % 5)
}

// asRateLimit extracts a *RateLimitError from err.
func asRateLimit(err error, target **RateLimitError) bool {
	var rl *RateLimitError
	if errors.As(err, &rl) {
		*target = rl
		return true
	}
	return false
}

// asLLMError extracts a *Error from err.
func asLLMError(err error, target **Error) bool {
	var e *Error
	if errors.As(err, &e) {
		*target = e
		return true
	}
	return false
}

// asExitError extracts an *exec.ExitError from err.
func asExitError(err error, target **exec.ExitError) bool {
	var ee *exec.ExitError
	if errors.As(err, &ee) {
		*target = ee
		return true
	}
	return false
}
