package email_test

// TestDialerOracle is the generator and guard for the IMAP transport oracle:
// it drives the real NetIMAPDialer against the package's fake server and writes
// the command transcript plus every caller-visible value to
// rust-tests/parity/oracle/imap-transport/transcript_cases.json.
//
// Regenerate with:
//
//	GOTOOLCHAIN=go1.26.6 go test ./internal/email -run TestDialerOracle -update
//
// Without -update the test verifies that the fixture still matches the current
// package, so the recorded bytes cannot drift away from the Go side.
//
// The transcript is normalised: the leading tag token is replaced by "<tag>".
// Tag numbering is per connection and no server contract depends on it, so the
// case pins the command sequence and its arguments instead.
//
// A case whose `replay` field is "go-only" is recorded but not replayable byte
// for byte in the Rust test; the reason is always stated in `replay_reason`.

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
)

var updateOracle = flag.Bool("update", false, "rewrite the IMAP transport oracle fixture")

const dialerFixturePath = "../../rust-tests/parity/oracle/imap-transport/transcript_cases.json"

type dialerCase struct {
	Name          string            `json:"name"`
	ServerMode    string            `json:"server_mode"`
	UIDValidity   uint32            `json:"uid_validity"`
	Messages      []fakeMessageWire `json:"messages"`
	Config        dialerConfigWire  `json:"config"`
	Ops           []string          `json:"ops"`
	Transcript    []string          `json:"transcript"`
	UIDValidityth *uint32           `json:"selected_uid_validity,omitempty"`
	SearchUIDs    []uint32          `json:"search_uids,omitempty"`
	Fetched       []fetchedWireCase `json:"fetched,omitempty"`
	Error         string            `json:"error,omitempty"`
	Replay        string            `json:"replay"`
	ReplayReason  string            `json:"replay_reason,omitempty"`
}

type fakeMessageWire struct {
	UID     uint32   `json:"uid"`
	Flags   []string `json:"flags,omitempty"`
	Header  string   `json:"header"`
	Body    string   `json:"body"`
	Invalid bool     `json:"invalid,omitempty"`
}

type dialerConfigWire struct {
	UseTLS               bool    `json:"use_tls"`
	AllowInsecureAuth    bool    `json:"allow_insecure_cleartext_auth"`
	Username             string  `json:"username"`
	Password             string  `json:"password,omitempty"`
	OAuth2Username       string  `json:"oauth2_username,omitempty"`
	OAuth2AccessToken    string  `json:"oauth2_access_token,omitempty"`
	TimeoutSeconds       float64 `json:"timeout_seconds"`
	RequireUserOnServer  string  `json:"server_require_user,omitempty"`
	RequirePassOnServer  string  `json:"server_require_pass,omitempty"`
	RequireTokenOnServer string  `json:"server_require_token,omitempty"`
}

type fetchedWireCase struct {
	UID     uint32   `json:"uid"`
	Flags   []string `json:"flags"`
	Header  string   `json:"header"`
	Body    string   `json:"body"`
	DateSet bool     `json:"internal_date_set"`
}

type dialerDocument struct {
	SourceRevision string       `json:"source_revision"`
	SourcePath     string       `json:"source_path"`
	Cases          []dialerCase `json:"cases"`
}

func TestDialerOracle(t *testing.T) {
	document := dialerDocument{
		SourceRevision: dialerOracleRevision,
		SourcePath:     "internal/email/dialer.go",
		Cases:          dialerCases(t),
	}
	raw, err := json.MarshalIndent(document, "", "  ")
	if err != nil {
		t.Fatalf("encode fixture: %v", err)
	}
	raw = append(raw, '\n')
	if *updateOracle {
		if err := os.MkdirAll(filepath.Dir(dialerFixturePath), 0o755); err != nil {
			t.Fatalf("create fixture dir: %v", err)
		}
		if err := os.WriteFile(dialerFixturePath, raw, 0o644); err != nil {
			t.Fatalf("write fixture: %v", err)
		}
		t.Logf("wrote %s (%d bytes)", dialerFixturePath, len(raw))
		return
	}
	frozen, err := os.ReadFile(dialerFixturePath)
	if err != nil {
		t.Fatalf("read fixture (run with -update to create it): %v", err)
	}
	if string(frozen) != string(raw) {
		t.Fatalf("fixture drifted from the current package; review the diff and regenerate with -update")
	}
}

// dialerOracleRevision pins the commit the recorded bytes were produced from.
const dialerOracleRevision = "5cd0cb60e08f27b3e2e0e0a1b90e0d9dd8f0d5a1"

func dialerCases(t *testing.T) []dialerCase {
	t.Helper()
	header := oracleHeader
	standard := []fakeMessage{
		{UID: 1, Header: header("First", "<m1@example.com>", "", "Mon, 21 Jul 2026 10:00:00 +0000"), Body: "first body", Flags: []string{"\\Seen"}},
		{UID: 2, Header: header("Second", "<m2@example.com>", "", "Mon, 21 Jul 2026 11:00:00 +0000"), Body: "second body"},
	}
	var cases []dialerCase
	run := func(name, mode string, messages []fakeMessage, cfg dialerConfigWire, ops []string, replay string) {
		cases = append(cases, measureDialer(t, name, mode, messages, cfg, ops, replay, ""))
	}

	run("plain_login_select_search_fetch", "plain", standard, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5,
		AllowInsecureAuth: true, RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "search:1:*", "fetch:1,2", "close"}, "byte")

	run("cleartext_auth_refused_without_starttls", "plain", standard, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5,
	}, []string{"select:INBOX"}, "byte")

	run("login_failure_is_reported", "plain", standard, dialerConfigWire{
		Username: "testuser", Password: "wrong", TimeoutSeconds: 5, AllowInsecureAuth: true,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX"}, "byte")

	run("xoauth2_sasl_ir", "plain", standard, dialerConfigWire{
		Username: "testuser", OAuth2Username: "testuser", OAuth2AccessToken: "testtoken",
		TimeoutSeconds: 5, AllowInsecureAuth: true, RequireTokenOnServer: "testtoken",
	}, []string{"select:INBOX", "search:1:*", "close"}, "byte")

	run("select_unknown_folder_creates_it_server_side", "plain", standard, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5, AllowInsecureAuth: true,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:Missing"}, "byte")

	run("search_then_empty_result", "plain", nil, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5, AllowInsecureAuth: true,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "search:1:*"}, "byte")

	run("invalid_uid_range", "plain", standard, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5, AllowInsecureAuth: true,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "search:not-a-range"}, "byte")

	run("fetch_returns_bounded_sections", "plain", standard, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5, AllowInsecureAuth: true,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "search:1:*", "fetch:1", "close"}, "byte")

	bigBody := strings.Repeat("x", 64*1024+1)
	run("oversized_body_section_is_refused", "plain", []fakeMessage{
		{UID: 1, Header: header("Big", "<big@example.com>", "", "Mon, 21 Jul 2026 10:00:00 +0000"), Body: bigBody},
	}, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5, AllowInsecureAuth: true,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "search:1:*", "fetch:1"}, "byte")

	// The TLS cases are replayable: the fixture records the command transcript
	// and the results, never certificate material, and each side mints and
	// trusts its own localhost certificate for the run. The Go side uses the
	// package's test certificate, the Rust side mints one in its own test.
	cases = append(cases, measureDialer(t, "starttls_login", "starttls", standard, dialerConfigWire{
		Username: "testuser", Password: "testpass", TimeoutSeconds: 5,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "search:1:*", "fetch:1", "close"}, "byte",
		"the certificate is minted per run on each side; the fixture carries no certificate material"))
	cases = append(cases, measureDialer(t, "implicit_tls_login", "tls", standard, dialerConfigWire{
		UseTLS: true, Username: "testuser", Password: "testpass", TimeoutSeconds: 5,
		RequireUserOnServer: "testuser", RequirePassOnServer: "testpass",
	}, []string{"select:INBOX", "close"}, "byte",
		"same certificate boundary as starttls_login"))
	return cases
}

func measureDialer(t *testing.T, name, mode string, messages []fakeMessage, cfg dialerConfigWire, ops []string, replay, replayReason string) dialerCase {
	t.Helper()
	server, err := startFakeIMAPServerMode(mode == "tls", mode == "starttls")
	if err != nil {
		t.Fatalf("%s: start server: %v", name, err)
	}
	defer server.close()
	server.folders["INBOX"] = &fakeFolder{UIDValidity: 100, Messages: messages}
	server.folders["Missing"] = &fakeFolder{UIDValidity: 7, Messages: messages}

	config := email.IMAPConfig{
		Host:                       "127.0.0.1",
		Port:                       server.port,
		Username:                   cfg.Username,
		Password:                   cfg.Password,
		UseTLS:                     cfg.UseTLS,
		AllowInsecureCleartextAuth: cfg.AllowInsecureAuth,
		Timeout:                    time.Duration(cfg.TimeoutSeconds * float64(time.Second)),
	}
	if cfg.OAuth2AccessToken != "" {
		config.OAuth2 = &email.OAuth2Token{Username: cfg.OAuth2Username, AccessToken: cfg.OAuth2AccessToken}
	}
	dialer := &email.NetIMAPDialer{}
	if server.clientTLS != nil {
		dialer.TLSConfig = server.clientTLS
	}

	recorded := dialerCase{
		Name:         name,
		ServerMode:   mode,
		UIDValidity:  100,
		Config:       cfg,
		Ops:          ops,
		Replay:       replay,
		ReplayReason: replayReason,
	}
	for _, message := range messages {
		recorded.Messages = append(recorded.Messages, fakeMessageWire(message))
	}

	session, err := dialer.Dial(context.Background(), config)
	if err != nil {
		recorded.Error = err.Error()
		recorded.Transcript = normaliseTranscript(server.getTranscript())
		return recorded
	}
	for _, op := range ops {
		command, argument, _ := strings.Cut(op, ":")
		switch command {
		case "select":
			uidValidity, opErr := session.Select(context.Background(), argument)
			if opErr != nil {
				recorded.Error = opErr.Error()
			} else {
				recorded.UIDValidityth = &uidValidity
			}
		case "search":
			uids, opErr := session.SearchUID(context.Background(), argument)
			if opErr != nil {
				recorded.Error = opErr.Error()
			} else {
				recorded.SearchUIDs = uids
				if recorded.SearchUIDs == nil {
					recorded.SearchUIDs = []uint32{}
				}
			}
		case "fetch":
			uids := parseUIDList(argument)
			fetched, opErr := session.Fetch(context.Background(), uids)
			if opErr != nil {
				recorded.Error = opErr.Error()
			} else {
				for _, message := range fetched {
					recorded.Fetched = append(recorded.Fetched, fetchedWireCase{
						UID: message.UID, Flags: message.Flags, Header: string(message.Header), Body: string(message.Body),
						DateSet: !message.InternalDate.IsZero(),
					})
				}
				if recorded.Fetched == nil {
					recorded.Fetched = []fetchedWireCase{}
				}
			}
		case "close":
			if opErr := session.Close(); opErr != nil {
				recorded.Error = opErr.Error()
			}
		default:
			t.Fatalf("%s: unknown op %q", name, op)
		}
		if recorded.Error != "" {
			break
		}
	}
	recorded.Transcript = normaliseTranscript(server.getTranscript())
	return recorded
}

// oracleHeader mirrors the header shape the other oracles use: a subset of the
// RFC 5322 fields the parser reads, with an explicit blank line.
func oracleHeader(subject, messageID, references, date string) string {
	lines := []string{"Subject: " + subject}
	if messageID != "" {
		lines = append(lines, "Message-ID: "+messageID)
	}
	if references != "" {
		lines = append(lines, "References: "+references)
	}
	if date != "" {
		lines = append(lines, "Date: "+date)
	}
	return strings.Join(lines, "\r\n") + "\r\n\r\n"
}

func parseUIDList(argument string) []uint32 {
	parts := strings.Split(argument, ",")
	out := make([]uint32, 0, len(parts))
	for _, part := range parts {
		var value uint64
		if _, err := fmt.Sscanf(strings.TrimSpace(part), "%d", &value); err == nil {
			out = append(out, uint32(value))
		}
	}
	return out
}

func normaliseTranscript(lines []string) []string {
	out := make([]string, 0, len(lines))
	for _, line := range lines {
		if _, rest, ok := strings.Cut(line, " "); ok {
			out = append(out, "<tag> "+rest)
			continue
		}
		out = append(out, line)
	}
	if out == nil {
		out = []string{}
	}
	return out
}
