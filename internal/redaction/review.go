package redaction

import (
	"bytes"
	"fmt"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

// Action is the decision for one detected match. Keep and Skip both retain
// the original bytes; Skip is named separately to preserve the review UI's
// y/n/s vocabulary and to make that choice observable in ReviewResult.
type Action string

// Decision is an alias used by adapters that model a review response as a
// decision rather than an action.
type Decision = Action

const (
	ActionKeep   Action = "keep"
	ActionRedact Action = "redact"
	ActionSkip   Action = "skip"
	ActionQuit   Action = "quit"

	// Short aliases make adapters straightforward while retaining the explicit
	// Action-prefixed constants for callers that prefer namespaced constants.
	Keep         = ActionKeep
	RedactAction = ActionRedact
	Skip         = ActionSkip
	Quit         = ActionQuit
)

// DecisionFunc is the testable boundary for an interactive review surface.
// Implementations may render a TUI and return a decision, while unit tests can
// return a deterministic sequence without opening a TTY.
type DecisionFunc func(Match) Action

// ReviewResult contains the transformed bytes and the decisions made before
// a possible quit. Content is always based on the original input; unmatched
// bytes and kept/skipped matches are copied byte-for-byte.
type ReviewResult struct {
	Content  []byte
	Changed  bool
	Quit     bool
	Redacted int
	Kept     int
	Skipped  int
}

// Review applies decisions to already-collected, non-overlapping matches.
// ActionQuit stops at that match, returning changes made earlier; the caller
// decides whether to persist those partial changes, exactly as interactive.py
// asks whether to save after q. A nil decision function keeps every match.
func Review(content []byte, matches []Match, decide DecisionFunc) (ReviewResult, error) {
	result := ReviewResult{Content: append([]byte(nil), content...)}
	if len(matches) == 0 {
		return result, nil
	}
	position := 0
	var out bytes.Buffer
	out.Grow(len(content))
	for index, match := range matches {
		if match.Start < position || match.Start < 0 || match.End < match.Start || match.End > len(content) {
			return ReviewResult{}, fmt.Errorf("invalid match %d: [%d:%d]", index, match.Start, match.End)
		}
		decision := ActionKeep
		if decide != nil {
			decision = decide(match)
		}
		switch decision {
		case ActionKeep:
			result.Kept++
		case ActionSkip:
			result.Skipped++
		case ActionRedact:
			out.Write(content[position:match.Start])
			out.WriteString(match.Replacement())
			position = match.End
			result.Redacted++
			result.Changed = true
			continue
		case ActionQuit:
			result.Quit = true
		default:
			return ReviewResult{}, fmt.Errorf("invalid review action %q for match %d", decision, index)
		}
		if decision == ActionQuit {
			break
		}
	}
	out.Write(content[position:])
	if result.Changed {
		result.Content = out.Bytes()
	}
	return result, nil
}

// ReviewText is the string convenience wrapper for Review. Match offsets must
// still refer to the UTF-8 byte representation of content.
func ReviewText(content string, matches []Match, decide DecisionFunc) (string, ReviewResult, error) {
	result, err := Review([]byte(content), matches, decide)
	return string(result.Content), result, err
}

// ReviewMatches collects matches and applies a deterministic decision
// function. It is the no-TTY entry point used by command adapters and tests.
func ReviewMatches(content []byte, profile *identity.Profile, decide DecisionFunc) (ReviewResult, error) {
	return Review(content, CollectMatches(string(content), profile), decide)
}

// ReviewTextWithProfile is the string equivalent of ReviewMatches.
func ReviewTextWithProfile(content string, profile *identity.Profile, decide DecisionFunc) (string, ReviewResult, error) {
	result, err := ReviewMatches([]byte(content), profile, decide)
	return string(result.Content), result, err
}
