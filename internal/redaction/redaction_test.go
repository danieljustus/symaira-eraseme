package redaction

import (
	"bytes"
	_ "embed"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

// The corpus is copied into this package so the Go port has an independent
// drift guard; existing Python fixtures remain untouched.
//
//go:embed testdata/golden.json
var goldenCorpus []byte

type goldenCase struct {
	Name     string `json:"name"`
	Input    string `json:"input"`
	Expected string `json:"expected"`
}

func TestGoldenCorpusMatchesPythonRedaction(t *testing.T) {
	var cases []goldenCase
	if err := json.Unmarshal(goldenCorpus, &cases); err != nil {
		t.Fatal(err)
	}
	if len(cases) != 9 {
		t.Fatalf("golden corpus has %d cases, want 9", len(cases))
	}
	for _, tc := range cases {
		t.Run(tc.Name, func(t *testing.T) {
			if got := Redact(tc.Input); got != tc.Expected {
				t.Fatalf("got %q, want %q", got, tc.Expected)
			}
		})
	}
}

func TestCollectMatchesUsesByteOffsetsAndResolvesOverlaps(t *testing.T) {
	content := "prefix test-123-4567@example.com suffix"
	matches := CollectMatches(content)
	if len(matches) != 1 {
		t.Fatalf("got %d matches: %#v", len(matches), matches)
	}
	match := matches[0]
	if match.Name != "Email" || content[match.Start:match.End] != "test-123-4567@example.com" {
		t.Fatalf("match = %#v", match)
	}
	if match.Start != len("prefix ") {
		t.Fatalf("start = %d, want %d", match.Start, len("prefix "))
	}
}

func TestProfileMatchesAreCaseInsensitiveAndProfileWinsOverlap(t *testing.T) {
	profile := &identity.Profile{
		FullName:       "Jane Doe",
		EmailAddresses: []string{"jane.doe@example.com"},
		PhoneNumbers:   []string{"555-123-4567"},
		Addresses:      []identity.Address{{Street: "Main Street 5", City: "Berlin", PostalCode: "10115"}},
	}
	content := "jane.doe@EXAMPLE.com / JANE DOE / 555-123-4567 / MAIN STREET 5"
	matches := CollectMatches(content, profile)
	if len(matches) != 4 {
		t.Fatalf("got %d matches: %#v", len(matches), matches)
	}
	wantNames := []string{"Profile Email", "Profile Name", "Profile Phone", "Profile Street"}
	for i, want := range wantNames {
		if matches[i].Name != want {
			t.Errorf("match %d name = %q, want %q", i, matches[i].Name, want)
		}
	}
	want := "[REDACTED-EMAIL] / [REDACTED-NAME] / [REDACTED-PHONE] / [REDACTED-STREET]"
	if got := Redact(content, profile); got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestInvalidSSNsAndUnhintedPassportsAreNotRedacted(t *testing.T) {
	content := "000-12-3456 666-12-3456 900-12-3456 123-00-6789 123-45-0000 AB123456"
	if got := Redact(content); got != content {
		t.Fatalf("invalid values changed: %q", got)
	}
}

func TestRedactPreservesUnmatchedBytes(t *testing.T) {
	content := string([]byte{0xff, 0x00}) + " before é jane@example.com after " + string([]byte{0xfe})
	got := []byte(Redact(content))
	want := append([]byte{0xff, 0x00}, []byte(" before é j**e@e*.com after ")...)
	want = append(want, 0xfe)
	if !bytes.Equal(got, want) {
		t.Fatalf("bytes changed:\n got % x\nwant % x", got, want)
	}
}

func TestWorkspacePathConfinement(t *testing.T) {
	root := t.TempDir()
	inside := filepath.Join(root, "inside.txt")
	if err := os.WriteFile(inside, []byte("jane@example.com"), 0o600); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(filepath.Dir(root), "outside.txt")
	if err := os.WriteFile(outside, []byte("outside@example.com"), 0o600); err != nil {
		t.Fatal(err)
	}
	defer os.Remove(outside)

	got, err := ReadWorkspaceFile("inside.txt", root)
	if err != nil || string(got) != "jane@example.com" {
		t.Fatalf("inside read = %q, %v", got, err)
	}
	for _, path := range []string{"../outside.txt", outside, root + "-sibling/out.txt"} {
		if _, err := ReadWorkspaceFile(path, root); !errors.Is(err, ErrPathOutsideWorkspace) {
			t.Errorf("path %q error = %v, want ErrPathOutsideWorkspace", path, err)
		}
	}
	if _, err := ReadWorkspaceFile("missing.txt", root); !errors.Is(err, os.ErrNotExist) {
		t.Errorf("missing error = %v, want not exist", err)
	}
}

func TestWorkspacePathRejectsSymlinkEscape(t *testing.T) {
	root := t.TempDir()
	outside := filepath.Join(filepath.Dir(root), "symlink-target.txt")
	if err := os.WriteFile(outside, []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	defer os.Remove(outside)
	link := filepath.Join(root, "link.txt")
	if err := os.Symlink(outside, link); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}
	if _, err := ReadWorkspaceFile("link.txt", root); !errors.Is(err, ErrPathOutsideWorkspace) {
		t.Fatalf("symlink error = %v, want ErrPathOutsideWorkspace", err)
	}
}

func TestRedactFileReadsOnlyConfinedWorkspace(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "message.txt")
	if err := os.WriteFile(path, []byte("Contact jane@example.com"), 0o600); err != nil {
		t.Fatal(err)
	}
	got, err := RedactFile("message.txt", root)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "Contact j**e@e*.com" {
		t.Fatalf("got %q", got)
	}
}

func TestReviewSupportsKeepRedactSkipAndQuitWithoutTTY(t *testing.T) {
	content := []byte("a jane@example.com b 555-123-4567 c alice@example.com d")
	matches := CollectMatches(string(content))
	decisions := []Action{ActionKeep, ActionRedact, ActionSkip}
	decisionIndex := 0
	result, err := Review(content, matches, func(Match) Action {
		decision := decisions[decisionIndex]
		decisionIndex++
		return decision
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.Quit || result.Redacted != 1 || result.Kept != 1 || result.Skipped != 1 || !result.Changed {
		t.Fatalf("result = %+v", result)
	}
	if string(result.Content) != "a jane@example.com b ***-***-4567 c alice@example.com d" {
		t.Fatalf("content = %q", result.Content)
	}

	quitResult, err := Review(content, matches, func(match Match) Action {
		if strings.Contains(match.Value, "jane") {
			return ActionRedact
		}
		return ActionQuit
	})
	if err != nil {
		t.Fatal(err)
	}
	if !quitResult.Quit || string(quitResult.Content) != "a j**e@e*.com b 555-123-4567 c alice@example.com d" {
		t.Fatalf("quit result = %+v", quitResult)
	}
}

func TestReviewRejectsInvalidMatchAndAction(t *testing.T) {
	if _, err := Review([]byte("abc"), []Match{{Start: 0, End: 4, Value: "abc"}}, nil); err == nil {
		t.Fatal("expected invalid match error")
	}
	_, err := Review([]byte("abc"), []Match{{Start: 0, End: 1, Value: "a"}}, func(Match) Action { return "invalid" })
	if err == nil {
		t.Fatal("expected invalid action error")
	}
}
