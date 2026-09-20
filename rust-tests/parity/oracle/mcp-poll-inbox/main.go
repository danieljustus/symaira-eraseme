// Command mcp-poll-inbox is the byte oracle for the MCP `poll_inbox` tool.
//
// The tool's transport is injectable — `mcp.ContractHandlerOptions` carries the
// `IMAPDialer` and the `HWMStore`, exactly as Go documents for tests — so this
// oracle drives the REAL contract handler with a scripted dialer instead of a
// live mailbox. The mailbox script is a language-neutral JSON file
// (`tests/fixtures/mcp-contract/mcp-003-poll/mailbox.json`) that the Rust replay
// test reads as well: both sides script the same mailbox, so a difference in
// the answer is a port defect and not a difference in the fake.
//
// Deliberate limits of the script (documented in the fixture itself):
//   - `SearchUID` ignores its `since` argument. The policy computes that window
//     from the wall clock, so honouring it would make the recorded answer move.
//   - Every folder has a fixed `uid_validity`, so a fresh high-water-mark store
//     always cold-starts and the fetched UID range is stable.
package main

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"

	_ "modernc.org/sqlite"
)

const (
	sourceRevision = "79bf23e83b31f18d98487101200eaf32749e5a46"
	sourcePath     = "internal/mcp/poll_inbox.go"
	pollFixtureDir = "tests/fixtures/mcp-contract/mcp-003-poll"
)

type mailboxMessage struct {
	UID          uint32   `json:"uid"`
	Flags        []string `json:"flags"`
	InternalDate string   `json:"internal_date"`
	Header       string   `json:"header"`
	Body         string   `json:"body"`
}

type mailboxFolder struct {
	UIDValidity uint32           `json:"uid_validity"`
	Messages    []mailboxMessage `json:"messages"`
}

type mailboxScript struct {
	Schema  string                   `json:"schema"`
	Folders map[string]mailboxFolder `json:"folders"`
}

type fixtureCase struct {
	Name       string  `json:"name"`
	Request    string  `json:"request,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
}

// scriptedSession replays one folder of the mailbox script.
type scriptedSession struct {
	script mailboxScript
	folder mailboxFolder
}

func (s *scriptedSession) Select(_ context.Context, folder string) (uint32, error) {
	selected, ok := s.script.Folders[folder]
	if !ok {
		return 0, fmt.Errorf("scripted mailbox has no folder %q", folder)
	}
	s.folder = selected
	return selected.UIDValidity, nil
}

func (s *scriptedSession) SearchUID(_ context.Context, uidRange string, _ ...time.Time) ([]uint32, error) {
	low, high := uidBounds(uidRange)
	out := make([]uint32, 0, len(s.folder.Messages))
	for _, message := range s.folder.Messages {
		if message.UID < low || message.UID > high {
			continue
		}
		out = append(out, message.UID)
	}
	return out, nil
}

func (s *scriptedSession) Fetch(_ context.Context, uids []uint32) ([]email.FetchedMessage, error) {
	byUID := make(map[uint32]mailboxMessage, len(s.folder.Messages))
	for _, message := range s.folder.Messages {
		byUID[message.UID] = message
	}
	out := make([]email.FetchedMessage, 0, len(uids))
	for _, uid := range uids {
		message, ok := byUID[uid]
		if !ok {
			return nil, fmt.Errorf("scripted mailbox has no uid %d", uid)
		}
		internalDate, err := time.Parse(time.RFC3339, message.InternalDate)
		if err != nil {
			return nil, err
		}
		out = append(out, email.FetchedMessage{
			UID:          message.UID,
			Header:       []byte(message.Header),
			Body:         []byte(message.Body),
			Flags:        message.Flags,
			InternalDate: internalDate,
		})
	}
	return out, nil
}

func (s *scriptedSession) Close() error { return nil }

type scriptedDialer struct {
	script mailboxScript
}

func (d *scriptedDialer) Dial(_ context.Context, _ email.IMAPConfig) (email.IMAPSession, error) {
	return &scriptedSession{script: d.script}, nil
}

// uidBounds reads the policy's `<low>:<high>` range. `*` is the open end.
func uidBounds(uidRange string) (uint32, uint32) {
	parts := strings.SplitN(uidRange, ":", 2)
	low := uint32(1)
	high := uint32(^uint32(0))
	if len(parts) > 0 && parts[0] != "*" && parts[0] != "" {
		if value, err := strconv.ParseUint(parts[0], 10, 32); err == nil {
			low = uint32(value)
		}
	}
	if len(parts) > 1 && parts[1] != "*" && parts[1] != "" {
		if value, err := strconv.ParseUint(parts[1], 10, 32); err == nil {
			high = uint32(value)
		}
	}
	return low, high
}

func callRequest(id int, name string, arguments string) string {
	return fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"%s","arguments":%s}}`, id, name, arguments)
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		writeFixture()
		return
	}
	raw, err := io.ReadAll(os.Stdin)
	if err != nil {
		fail(err)
	}
	var output bytes.Buffer
	server := mcp.NewServer(mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{
		IMAPDialer: &scriptedDialer{script: loadMailbox()},
		HWMStore:   email.NewMemoryHWMStore(),
	}))
	if err := server.ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output); err != nil {
		fail(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(struct {
		BodyB64 string `json:"body_b64"`
	}{BodyB64: base64Encode(output.Bytes())}); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

func base64Encode(raw []byte) string {
	const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
	var out strings.Builder
	for index := 0; index < len(raw); index += 3 {
		var chunk [3]byte
		remaining := len(raw) - index
		copied := copy(chunk[:], raw[index:])
		value := uint32(chunk[0])<<16 | uint32(chunk[1])<<8 | uint32(chunk[2])
		out.WriteByte(alphabet[(value>>18)&0x3f])
		out.WriteByte(alphabet[(value>>12)&0x3f])
		if remaining > 1 {
			out.WriteByte(alphabet[(value>>6)&0x3f])
		} else {
			out.WriteByte('=')
		}
		if remaining > 2 {
			out.WriteByte(alphabet[value&0x3f])
		} else if copied > 0 {
			out.WriteByte('=')
		}
	}
	return out.String()
}

func loadMailbox() mailboxScript {
	raw, err := os.ReadFile(filepath.Join(pollFixtureDir, "mailbox.json"))
	if err != nil {
		fail(err)
	}
	var script mailboxScript
	if err := json.Unmarshal(raw, &script); err != nil {
		fail(err)
	}
	return script
}

// writeFixture records every case inside its own isolated workspace: a fresh
// data directory (so the developer's store can never leak in), a private
// `XDG_CONFIG_HOME`, a store the handler creates itself, and then the frozen
// rows. Each case gets a fresh high-water-mark store, so no case can influence
// the next one.
func writeFixture() {
	seed, err := os.ReadFile(filepath.Join(pollFixtureDir, "seed.sql"))
	if err != nil {
		fail(err)
	}
	script := loadMailbox()

	cases := []struct {
		name      string
		arguments string
	}{
		{
			name:      "poll_inbox_reports_an_empty_mailbox",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":["Empty"]}`,
		},
		{
			// The INBOX reply carries `In-Reply-To: <sent-1@example.com>`, which is
			// the Message-ID frozen on request 1's SENT event: it must match by
			// thread, and the unrelated newsletter must stay unmatched.
			name:      "poll_inbox_matches_a_thread_reply_and_leaves_a_newsletter_unmatched",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":["INBOX"]}`,
		},
		{
			// Request 2 has no recorded Message-ID, so only the normalized
			// subject ("Data Deletion Request — broker-b") can match it.
			name:      "poll_inbox_matches_by_subject_when_no_thread_is_recorded",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":["Archive"]}`,
		},
		{
			name:      "poll_inbox_aggregates_several_folders_and_deduplicates",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":["Empty","INBOX","Archive"]}`,
		},
		{
			// No `folders` at all: Go falls back to cfg.Folder and then to INBOX.
			name:      "poll_inbox_defaults_to_inbox_without_a_folder_argument",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true}`,
		},
		{
			// The scripted mailbox knows no such folder, so the transport fails
			// and the handler error reaches the client sanitized.
			name:      "poll_inbox_reports_a_transport_failure",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":["NoSuchFolder"]}`,
		},
		{
			// The policy drops fetched messages whose Date header is older than
			// the window (it does not merely narrow the UID search). The fixture
			// messages are dated 2026-08-08, so a one-day window must fetch
			// nothing and a ten-year window must fetch them.
			name:      "poll_inbox_drops_messages_outside_the_since_window",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":1,"ssl":true,"folders":["INBOX"]}`,
		},
		{
			// The catalogue marks `port` required, so the server rejects the call
			// before the handler ever sees it.
			name:      "poll_inbox_rejects_a_request_without_the_required_port",
			arguments: `{"host":"imap.example.invalid","username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":["INBOX"]}`,
		},
		{
			// `folders` is typed as an array, so the string forms the handler
			// tolerates are unreachable through MCP — the type gate fires first.
			name:      "poll_inbox_rejects_folders_that_are_not_an_array",
			arguments: `{"host":"imap.example.invalid","port":993,"username":"oracle@example.invalid","since_days":3650,"ssl":true,"folders":"INBOX"}`,
		},
	}

	workspace, err := os.MkdirTemp("", "mcp-poll-inbox")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(workspace)

	dataDir := filepath.Join(workspace, "data")
	for _, dir := range []string{dataDir, filepath.Join(workspace, "xdg")} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			fail(err)
		}
	}
	if err := os.Setenv("SYMERASEME_DATA_DIR", dataDir); err != nil {
		fail(err)
	}
	if err := os.Setenv("SYMERASEME_DB_DIR", dataDir); err != nil {
		fail(err)
	}
	if err := os.Setenv("XDG_CONFIG_HOME", filepath.Join(workspace, "xdg")); err != nil {
		fail(err)
	}

	origin, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	if err := os.Chdir(workspace); err != nil {
		fail(err)
	}
	defer func() { _ = os.Chdir(origin) }()

	// One request through the handler creates the store with its schema; the
	// answer itself is discarded.
	warmUp := mcp.NewServer(mcp.ContractHandler())
	if err := warmUp.ServeStdio(
		context.Background(),
		bytes.NewReader([]byte(callRequest(0, "list_requests", `{}`)+"\n")),
		&bytes.Buffer{},
	); err != nil {
		fail(err)
	}
	applySeed(filepath.Join(dataDir, "symeraseme.db"), seed)

	recorded := make([]fixtureCase, 0, len(cases))
	for index, testCase := range cases {
		record := fixtureCase{
			Name:    testCase.name,
			Request: callRequest(index+1, "poll_inbox", testCase.arguments),
		}
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{
			IMAPDialer: &scriptedDialer{script: script},
			HWMStore:   email.NewMemoryHWMStore(),
		}))
		runErr := server.ServeStdio(
			context.Background(),
			bytes.NewReader(append([]byte(record.Request), '\n')),
			&output,
		)
		if runErr != nil {
			record.ParseError = true
		} else if body := output.String(); body != "" {
			record.Response = &body
		}
		recorded = append(recorded, record)
	}

	target, err := os.Create(filepath.Join(origin, pollFixtureDir, "cases.json"))
	if err != nil {
		fail(err)
	}
	defer target.Close()
	encoder := json.NewEncoder(target)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Mailbox        string        `json:"mailbox"`
		Seed           string        `json:"seed"`
		Cases          []fixtureCase `json:"cases"`
	}{
		SourceRevision: sourceRevision,
		SourcePath:     sourcePath,
		Mailbox:        filepath.Join(pollFixtureDir, "mailbox.json"),
		Seed:           filepath.Join(pollFixtureDir, "seed.sql"),
		Cases:          recorded,
	}); err != nil {
		fail(err)
	}
}

// applySeed runs the frozen rows. The file holds one statement per line (no
// semicolons inside values), which keeps the Go and Rust readers identical.
func applySeed(dbPath string, seed []byte) {
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		fail(err)
	}
	defer db.Close()
	for _, line := range strings.Split(string(seed), "\n") {
		statement := strings.TrimSpace(line)
		if statement == "" || strings.HasPrefix(statement, "--") {
			continue
		}
		if _, err := db.Exec(statement); err != nil {
			fail(fmt.Errorf("seed %q: %w", statement, err))
		}
	}
}
