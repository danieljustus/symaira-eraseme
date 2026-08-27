// Package timeutil mirrors the Python symeraseme.core.datetime_utils
// behaviour: parse ISO timestamps emitted by both the Python write path
// ("T"-separated) and SQLite's datetime('now') default (space-separated),
// and return a UTC-anchored time.Time.  Unparseable values yield an error
// so callers can decide whether to skip the row.
package timeutil

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

// Layouts accepted on the read path.  Order matters: we try the most
// specific match first so the shorter space-separated layout does not
// shadow a longer T-separated one.
var isoLayouts = []string{
	"2006-01-02T15:04:05Z07:00",
	"2006-01-02 15:04:05Z07:00",
	time.RFC1123Z,
	time.RFC1123,
	time.RFC850,
	time.RFC3339,
}

// Parse parses a timestamp string into a UTC-anchored time.Time.
//
// The Python reference accepts three ISO flavours (no tz, with tz, space
// separated).  We keep the same set and add the RFC 1123/3339 fallbacks
// for inbox messages that leak in via other code paths.  Empty input
// returns a typed error (ErrEmpty) so callers can distinguish "missing"
// from "malformed".
var ErrEmpty = errors.New("timeutil: empty timestamp")

func Parse(s string) (time.Time, error) {
	s = strings.TrimSpace(s)
	if s == "" {
		return time.Time{}, ErrEmpty
	}
	for _, layout := range isoLayouts {
		if t, err := time.Parse(layout, s); err == nil {
			return t.UTC(), nil
		}
	}
	// Fall back to layouts without an explicit zone: the Python writer
	// emits bare "YYYY-MM-DDTHH:MM:SS" and SQLite emits the space variant.
	// Both are interpreted as UTC per docs/event-store.md §1.
	naiveLayouts := []string{
		"2006-01-02T15:04:05",
		"2006-01-02 15:04:05",
	}
	for _, layout := range naiveLayouts {
		if t, err := time.ParseInLocation(layout, s, time.UTC); err == nil {
			return t.UTC(), nil
		}
	}
	return time.Time{}, fmt.Errorf("timeutil: cannot parse %q", s)
}

// FormatISO writes a time.Time as a UTC ISO-8601 string with seconds
// precision and an explicit "+00:00" offset — the canonical output the
// Python projection emits (isoformat() on a UTC-aware datetime renders
// "+00:00", never "Z").  The conformance test enforces byte parity with
// tests/fixtures/event-store/golden-projection.json.
func FormatISO(t time.Time) string {
	return t.UTC().Format("2006-01-02T15:04:05+00:00")
}

// FormatSQL is the wall-clock UTC form used by the Python write path
// for both occurred_at and recorded_at.
func FormatSQL(t time.Time) string {
	return t.UTC().Format("2006-01-02T15:04:05")
}
