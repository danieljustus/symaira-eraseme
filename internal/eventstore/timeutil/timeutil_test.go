package timeutil

import (
	"errors"
	"testing"
	"time"
)

func TestParseAcceptsContractTimestampVariants(t *testing.T) {
	for _, input := range []string{
		"2026-08-31T12:34:56Z",
		"2026-08-31T14:34:56+02:00",
		"2026-08-31 12:34:56Z",
		"2026-08-31T12:34:56",
		"2026-08-31 12:34:56",
		time.Now().UTC().Format(time.RFC1123Z),
	} {
		got, err := Parse(input)
		if err != nil {
			t.Errorf("Parse(%q): %v", input, err)
			continue
		}
		if got.Location() != time.UTC {
			t.Errorf("Parse(%q) location = %v, want UTC", input, got.Location())
		}
	}
}

func TestParseRejectsEmptyAndMalformedValues(t *testing.T) {
	if _, err := Parse("   "); !errors.Is(err, ErrEmpty) {
		t.Fatalf("empty error = %v, want ErrEmpty", err)
	}
	if _, err := Parse("not-a-timestamp"); err == nil {
		t.Fatal("malformed timestamp accepted")
	}
}

func TestFormatUsesCanonicalUTCBytes(t *testing.T) {
	value := time.Date(2026, 8, 31, 14, 34, 56, 0, time.FixedZone("CEST", 2*60*60))
	if got := FormatISO(value); got != "2026-08-31T12:34:56+00:00" {
		t.Fatalf("FormatISO = %q", got)
	}
	if got := FormatSQL(value); got != "2026-08-31 12:34:56" {
		t.Fatalf("FormatSQL = %q", got)
	}
}
