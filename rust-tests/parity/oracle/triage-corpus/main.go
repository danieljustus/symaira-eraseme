// Command triage-corpus is the committed Go oracle for the shared broker-reply
// corpus (contract row DOM-004).
//
// The corpus is `tests/fixtures/broker_replies/*.txt`, which previously had no
// consumer at all. This oracle makes it executable evidence: it runs the real
// `internal/triage` functions over every corpus file and emits a fixture whose
// prompts are the bytes Go actually produced.
//
// The corpus files are *inputs*, not expectations. Nothing here reads a
// hand-written expected value; every recorded byte comes out of the Go code.
//
// With `--fixture` it writes the fixture to stdout. Without arguments it
// re-derives the same payload for a single case fed on stdin, so the Rust
// replay test can compare against a live Go process as well as the frozen file.
package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/triage"
)

const (
	schema = "symaira-eraseme.triage-corpus.v1"
	// CorpusDir is relative to the repository root.
	corpusDir = "tests/fixtures/broker_replies"
	// The Go sources whose behaviour this fixture pins.
	sourceClassifier = "internal/triage/classifier.go"
	sourceRebuttal   = "internal/triage/rebuttal.go"
	// The commit this fixture was generated from. Hardcoded rather than read
	// from `.git/HEAD`, which does not exist in the exported oracle tree the
	// generator builds with `git archive`. Follows the convention of the other
	// committed oracles, which pin a revision constant.
	sourceRevision = "9c97319f8e293b03e39e36ea502b8934cfa6f82e"
)

type fixture struct {
	Schema         string            `json:"schema"`
	OracleRevision string            `json:"oracle_revision"`
	SourceSHA256   map[string]string `json:"source_sha256"`
	Cases          []corpusCase      `json:"cases"`
}

type corpusCase struct {
	Name string `json:"name"`
	// The corpus file this case came from, with its digest, so the corpus
	// itself is covered by the provenance block rather than only the Go code.
	CorpusFile       string `json:"corpus_file"`
	CorpusSHA256     string `json:"corpus_sha256"`
	BrokerName       string `json:"broker_name"`
	BrokerWebsite    string `json:"broker_website"`
	ReplySubject     string `json:"reply_subject"`
	ReplyBody        string `json:"reply_body"`
	FallbackTemplate string `json:"fallback_template"`
	// The exact bytes Go's BuildUserPrompt returned, base64 so they survive
	// JSON round-tripping unchanged.
	PromptBase64 string `json:"prompt_base64"`
	// The same prompt's UTF-8 length, so a byte-level mismatch is obvious
	// without decoding.
	PromptBytes int `json:"prompt_bytes"`
}

func main() {
	root, err := repositoryRoot()
	if err != nil {
		fail(err)
	}
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		payload, err := buildFixture(root)
		if err != nil {
			fail(err)
		}
		encoder := json.NewEncoder(os.Stdout)
		encoder.SetIndent("", "  ")
		if err := encoder.Encode(payload); err != nil {
			fail(err)
		}
		return
	}
	// Single-case mode: read {name, broker_name, reply_subject, reply_body}
	// from stdin and emit one case, so the Rust test can compare against a
	// live Go process.
	raw, err := io.ReadAll(os.Stdin)
	if err != nil {
		fail(err)
	}
	var request corpusCase
	if err := json.Unmarshal(raw, &request); err != nil {
		fail(err)
	}
	encoder := json.NewEncoder(os.Stdout)
	if err := encoder.Encode(derive(request)); err != nil {
		fail(err)
	}
}

// repositoryRoot walks up until it finds the module root, so the oracle works
// both from the repository and from an exported oracle tree.
func repositoryRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", err
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir, nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("go.mod not found above the working directory")
		}
		dir = parent
	}
}

// derive runs the real Go functions for one corpus case. This is the only place
// the recorded bytes are produced.
func derive(input corpusCase) corpusCase {
	result := input
	prompt := []byte(triage.BuildUserPrompt(
		input.BrokerName, input.BrokerWebsite,
		"", "",
		input.ReplySubject, input.ReplyBody,
		nil,
	))
	result.PromptBase64 = base64.StdEncoding.EncodeToString(prompt)
	result.PromptBytes = len(prompt)
	result.FallbackTemplate = triage.SelectFallbackTemplate(input.ReplyBody)
	return result
}

func buildFixture(root string) (fixture, error) {
	entries, err := os.ReadDir(filepath.Join(root, corpusDir))
	if err != nil {
		return fixture{}, fmt.Errorf("read corpus: %w", err)
	}
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".txt") {
			names = append(names, entry.Name())
		}
	}
	// Sorted so the fixture is byte-stable across runs and filesystems.
	sort.Strings(names)
	if len(names) == 0 {
		return fixture{}, fmt.Errorf("corpus directory %s holds no .txt files", corpusDir)
	}

	payload := fixture{
		Schema:         schema,
		OracleRevision: sourceRevision,
		SourceSHA256:   map[string]string{},
		Cases:          make([]corpusCase, 0, len(names)),
	}
	for _, source := range []string{sourceClassifier, sourceRebuttal} {
		digest, err := fileDigest(filepath.Join(root, source))
		if err != nil {
			return fixture{}, err
		}
		payload.SourceSHA256[source] = digest
	}

	for _, name := range names {
		path := filepath.Join(root, corpusDir, name)
		raw, err := os.ReadFile(path)
		if err != nil {
			return fixture{}, fmt.Errorf("read %s: %w", path, err)
		}
		subject, body := splitReply(string(raw))
		c := corpusCase{
			Name:          strings.TrimSuffix(name, ".txt"),
			CorpusFile:    corpusDir + "/" + name,
			CorpusSHA256:  digestOf(raw),
			BrokerName:    brokerName(subject, body),
			BrokerWebsite: "",
			ReplySubject:  subject,
			ReplyBody:     body,
		}
		payload.Cases = append(payload.Cases, derive(c))
	}
	return payload, nil
}

// splitReply separates the corpus file into its subject header and the rest.
// The files are stored as they arrived, so the subject is a header line rather
// than a parsed field.
func splitReply(raw string) (subject, body string) {
	normalised := strings.ReplaceAll(raw, "\r\n", "\n")
	lines := strings.Split(normalised, "\n")
	rest := make([]string, 0, len(lines))
	for _, line := range lines {
		if strings.HasPrefix(line, "Subject:") {
			subject = strings.TrimSpace(strings.TrimPrefix(line, "Subject:"))
			continue
		}
		rest = append(rest, line)
	}
	return subject, strings.TrimSpace(strings.Join(rest, "\n"))
}

// brokerName derives the broker label the prompt uses. The corpus has no
// broker field, so the reply's domain is the only stable identifier it carries.
func brokerName(subject, body string) string {
	for _, line := range strings.Split(body, "\n") {
		if !strings.HasPrefix(line, "From:") {
			continue
		}
		address := strings.TrimSpace(strings.TrimPrefix(line, "From:"))
		if at := strings.LastIndex(address, "@"); at >= 0 {
			return address[at+1:]
		}
		return address
	}
	return "Unknown"
}

func fileDigest(path string) (string, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("read %s: %w", path, err)
	}
	return digestOf(raw), nil
}

func digestOf(raw []byte) string {
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
