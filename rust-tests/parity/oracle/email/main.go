// Command email is the byte oracle for the Go `internal/email` inbox policy
// (register row DOM-006: IMAP UIDVALIDITY/HWM/search/fetch policy).
//
// The IMAP side is a scripted session rather than a server: what is under test
// is which UID range is searched, which window is fetched, when the high-water
// mark advances, and what the caller sees. Every recorded answer comes from the
// production `email` package — only the transport boundary is scripted, exactly
// as the package's own tests do it.
//
// Usage:
//
//	GOTOOLCHAIN=go1.26.6 go run ./rust-tests/parity/oracle/email            # stdout
//	GOTOOLCHAIN=go1.26.6 go run ./rust-tests/parity/oracle/email --fixture  # writes the fixture
//
// The document is stable: running it twice must produce identical bytes.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
)

const (
	sourceRevision = "76759b80b5831227693dcccf1cb4a37ef9bbbe49"
	sourcePath     = "internal/email"
	fixturePath    = "rust-tests/parity/oracle/email/email_cases.json"
)

type normalizeCase struct {
	Name   string `json:"name"`
	Input  string `json:"input"`
	Output string `json:"output"`
}

type bodyCase struct {
	Name   string `json:"name"`
	Body   string `json:"body"`
	Max    int    `json:"max"`
	Output string `json:"output"`
}

type subjectCase struct {
	Name    string `json:"name"`
	Base    string `json:"base"`
	Reply   string `json:"reply"`
	Matches bool   `json:"matches"`
}

// parseCase pins ParseFetchedMessage: the header bytes a session hands over,
// and the Message the parser derives from them.
type parseCase struct {
	Name         string   `json:"name"`
	Header       string   `json:"header"`
	Body         string   `json:"body"`
	Flags        []string `json:"flags,omitempty"`
	InternalDate string   `json:"internal_date,omitempty"`
	Output       string   `json:"output,omitempty"`
	Error        string   `json:"error,omitempty"`
}

type matchCase struct {
	Name      string                 `json:"name"`
	Messages  []email.Message        `json:"messages"`
	Requests  []email.RemovalRequest `json:"requests"`
	ThreadMap map[string]int64       `json:"thread_map,omitempty"`
	Output    string                 `json:"output"`
}

// fetchedWire is the JSON form of one scripted FETCH result. Header and Body
// carry the raw bytes the scripted session returns for that UID.
type fetchedWire struct {
	UID          uint32   `json:"uid"`
	Header       string   `json:"header"`
	Body         string   `json:"body"`
	Flags        []string `json:"flags,omitempty"`
	InternalDate string   `json:"internal_date,omitempty"`
}

// folderScript is one folder's scripted session. Fetched answers a FETCH, Omit
// are UIDs the answer leaves out, and the error fields make the session fail at
// that point.
type folderScript struct {
	UIDValidity uint32        `json:"uid_validity"`
	UIDs        []uint32      `json:"uids"`
	Fetched     []fetchedWire `json:"fetched"`
	Omit        []uint32      `json:"omit,omitempty"`
	SelectError string        `json:"select_error,omitempty"`
	SearchError string        `json:"search_error,omitempty"`
	FetchError  string        `json:"fetch_error,omitempty"`
}

type hwmWire struct {
	Host        string  `json:"host"`
	Folder      string  `json:"folder"`
	UIDValidity *uint32 `json:"uid_validity"`
	LastUID     *uint32 `json:"last_uid"`
}

type searchWire struct {
	Folder   string `json:"folder"`
	UIDRange string `json:"uid_range"`
	SinceSet bool   `json:"since_set"`
}

type callWire struct {
	Messages string       `json:"messages"`
	Error    string       `json:"error,omitempty"`
	Searches []searchWire `json:"searches"`
	Fetches  [][]uint32   `json:"fetches"`
}

type pollCase struct {
	Name      string                  `json:"name"`
	Mode      string                  `json:"mode"`
	Host      string                  `json:"host"`
	Folder    string                  `json:"folder"`
	Max       int                     `json:"max_messages"`
	SinceDays int                     `json:"since_days"`
	Folders   []string                `json:"folders"`
	Initial   []hwmWire               `json:"initial_hwm"`
	Script    map[string]folderScript `json:"script"`
	Polls     int                     `json:"polls"`
	Calls     []callWire              `json:"calls"`
	Final     []hwmWire               `json:"final_hwm"`
}

type insertWire struct {
	RequestID *int64 `json:"request_id"`
	MessageID string `json:"message_id"`
	ThreadID  string `json:"thread_id"`
	From      string `json:"from"`
	Subject   string `json:"subject"`
	Snippet   string `json:"snippet"`
}

type serviceCase struct {
	Name      string                  `json:"name"`
	Host      string                  `json:"host"`
	Folder    string                  `json:"folder"`
	Folders   []string                `json:"folders"`
	Requests  []email.RemovalRequest  `json:"requests"`
	ThreadMap map[string]int64        `json:"thread_map,omitempty"`
	Initial   []hwmWire               `json:"initial_hwm"`
	Script    map[string]folderScript `json:"script"`
	Output    string                  `json:"output,omitempty"`
	Error     string                  `json:"error,omitempty"`
	Inserts   []insertWire            `json:"inserts"`
	Final     []hwmWire               `json:"final_hwm"`
}

type imapConfigCase struct {
	Name        string            `json:"name"`
	Env         map[string]string `json:"env"`
	AccessToken string            `json:"access_token,omitempty"`
	OAuthUser   string            `json:"oauth2_username,omitempty"`
	Config      string            `json:"config,omitempty"`
	Error       string            `json:"error,omitempty"`
}

type document struct {
	SourceRevision string           `json:"source_revision"`
	SourcePath     string           `json:"source_path"`
	Normalize      []normalizeCase  `json:"normalize_subject"`
	Bodies         []bodyCase       `json:"parse_email_body"`
	Subjects       []subjectCase    `json:"subject_matches"`
	Matches        []matchCase      `json:"match_reply_to_request"`
	Parses         []parseCase      `json:"parse_fetched_message"`
	Polls          []pollCase       `json:"poll_inbox"`
	Services       []serviceCase    `json:"poll_and_match"`
	Configs        []imapConfigCase `json:"imap_config"`
}

func main() {
	target := os.Stdout
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		file, err := os.Create(fixturePath)
		if err != nil {
			fail(err)
		}
		defer file.Close()
		target = file
	}
	encoder := json.NewEncoder(target)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(collect()); err != nil {
		fail(err)
	}
}

func collect() document {
	return document{
		SourceRevision: sourceRevision,
		SourcePath:     sourcePath,
		Normalize:      normalizeCases(),
		Bodies:         bodyCases(),
		Subjects:       subjectCases(),
		Matches:        matchCases(),
		Parses:         parseCases(),
		Polls:          pollCases(),
		Services:       serviceCases(),
		Configs:        configCases(),
	}
}

func normalizeCases() []normalizeCase {
	inputs := []string{
		"",
		"   ",
		"Data Deletion Request — broker-a",
		"Re: Data Deletion Request — broker-a",
		"RE:RE: Data Deletion Request — broker-a",
		"re : Data Deletion Request — broker-a",
		"Fwd: Re: Data Deletion Request — broker-a",
		"Aw: Data Deletion Request — broker-a",
		"ANTWORT: Data Deletion Request — broker-a",
		"Réf: Data Deletion Request — broker-a",
		"SV: Data Deletion Request — broker-a",
		"Vs: Data Deletion Request — broker-a",
		"WG: Data Deletion Request — broker-a",
		"Ref: Data Deletion Request — broker-a",
		"Re[2]: Data Deletion Request — broker-a",
		"Betreff: Data Deletion Request — broker-a",
		"Re:",
		"Data Deletion Request — broker-a (Re: verweigert)",
	}
	cases := make([]normalizeCase, 0, len(inputs))
	for index, input := range inputs {
		cases = append(cases, normalizeCase{
			Name:   fmt.Sprintf("normalize_%02d", index),
			Input:  input,
			Output: email.NormalizeSubject(input),
		})
	}
	return cases
}

func bodyCases() []bodyCase {
	inputs := []struct {
		body string
		max  int
	}{
		{"  removed  ", 200},
		{"", 200},
		{"   ", 200},
		{"abc", 0},
		{"abcdefghij", 5},
		{"üöä-üöä", 4},
		{"line one\r\nline two", 10},
		{strings.Repeat("x", 600), 500},
	}
	cases := make([]bodyCase, 0, len(inputs))
	for index, input := range inputs {
		cases = append(cases, bodyCase{
			Name:   fmt.Sprintf("body_%02d", index),
			Body:   input.body,
			Max:    input.max,
			Output: email.ParseEmailBody(input.body, input.max),
		})
	}
	return cases
}

func subjectCases() []subjectCase {
	inputs := []struct {
		base  string
		reply string
	}{
		{"Data Deletion Request — broker-a", "Re: Data Deletion Request — broker-a"},
		{"data deletion request — BROKER-A", "AW: Data deletion request — broker-a"},
		{"Data Deletion Request — broker-a", "Data Deletion Request — broker-b"},
		{"Data Deletion Request — broker-a", ""},
		{"", ""},
		{"Re: Data Deletion Request — broker-a", "Data Deletion Request — broker-a"},
	}
	cases := make([]subjectCase, 0, len(inputs))
	for index, input := range inputs {
		cases = append(cases, subjectCase{
			Name:    fmt.Sprintf("subject_%02d", index),
			Base:    input.base,
			Reply:   input.reply,
			Matches: email.SubjectMatches(input.base, input.reply),
		})
	}
	return cases
}

func parseCases() []parseCase {
	inputs := []parseCase{
		{
			Name:   "plain_headers",
			Header: "Subject: Data Deletion Request\r\nFrom: Broker <b@example.com>\r\nTo: me@example.com\r\nDate: Mon, 21 Jul 2026 10:00:00 +0200\r\nMessage-ID: <reply@example.com>\r\nReferences: <sent@example.com>\r\n\r\n",
			Body:   "removed",
			Flags:  []string{"\\Seen"},
		},
		{
			Name:   "folded_subject_and_references",
			Header: "Subject: Data Deletion\r\n Request — broker-a\r\nReferences: <first@example.com>\r\n	<second@example.com>\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_quoted_printable_utf8",
			Header: "Subject: =?UTF-8?Q?Re=3A_Data_Deletion_Request_=E2=80=94_broker-a?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_base64_utf8",
			Header: "Subject: =?UTF-8?B?UmU6IERhdGEgRGVsZXRpb24gUmVxdWVzdCDigJQgYnJva2VyLWE=?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_unknown_charset_is_kept_raw",
			Header: "Subject: =?ISO-8859-1?Q?Re=3A_Datenl=F6schung?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_broken_is_kept_raw",
			Header: "Subject: =?UTF-8?Q?Re=3A_Data?= and =?UTF-8?X?broken?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_windows_1252",
			Header: "Subject: =?windows-1252?Q?Re=3A_Datenl=F6schung_=93broker=94?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_invalid_hex",
			Header: "Subject: =?UTF-8?Q?Re=3A_=ZZbroken?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_missing_terminator",
			Header: "Subject: =?UTF-8?Q?Re=3A_Data\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_invalid_base64",
			Header: "Subject: =?UTF-8?B?not-base64!!?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "adjacent_encoded_words",
			Header: "Subject: =?UTF-8?Q?Re=3A_Data_?= =?UTF-8?Q?Deletion_Request?=\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_word_with_padding_text",
			Header: "Subject: [external] =?UTF-8?Q?Re=3A_Data_Deletion_Request?= (bitte lesen)\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "encoded_body_is_not_decoded",
			Header: "Subject: =?UTF-8?Q?plain?=\r\nFrom: =?UTF-8?B?QnJva2Vy?= <b@example.com>\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "no_message_id",
			Header: "Subject: Second\r\nFrom: Broker <b@example.com>\r\n\r\n",
		},
		{
			Name:         "internal_date_fallback",
			Header:       "Subject: Undated\r\nMessage-ID: <undated@example.com>\r\n\r\n",
			InternalDate: "2026-07-22T09:30:00Z",
		},
		{
			Name:         "unparsable_date_falls_back_to_the_internal_date",
			Header:       "Subject: Bad date\r\nDate: not a date\r\nMessage-ID: <bad@example.com>\r\n\r\n",
			InternalDate: "2026-07-23T08:00:00Z",
		},
		{
			Name:   "in_reply_to_is_the_thread_fallback",
			Header: "Subject: Re: Data Deletion Request — broker-a\r\nIn-Reply-To: <thread@example.com>\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "references_without_message_ids_fall_back",
			Header: "Subject: Re: Data Deletion Request — broker-a\r\nReferences: no-angle-brackets\r\nIn-Reply-To: <thread@example.com>\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "first_reference_wins",
			Header: "Subject: Re: Data Deletion Request — broker-a\r\nReferences: <one@example.com> <two@example.com>\r\nMessage-ID: <reply@example.com>\r\n\r\n",
		},
		{
			Name:   "empty_flags_stay_null",
			Header: "Subject: Flags\r\nMessage-ID: <flags@example.com>\r\n\r\n",
			Flags:  nil,
		},
		{
			Name:   "malformed_header_is_an_error",
			Header: "Subject Data Deletion Request\r\n\r\n",
		},
		{
			Name:   "folded_continuation_without_a_field_is_an_error",
			Header: "  dangling continuation\r\n\r\n",
		},
	}
	cases := make([]parseCase, 0, len(inputs))
	for index, input := range inputs {
		message := email.FetchedMessage{
			UID:    uint32(index + 1),
			Header: []byte(input.Header),
			Body:   []byte(input.Body),
			Flags:  input.Flags,
		}
		if input.InternalDate != "" {
			parsed, err := time.Parse(time.RFC3339, input.InternalDate)
			if err != nil {
				fail(err)
			}
			message.InternalDate = parsed
		}
		out := input
		parsed, err := email.ParseFetchedMessage(message)
		if err != nil {
			out.Error = err.Error()
		} else {
			out.Output = mustMarshal(parsed)
		}
		cases = append(cases, out)
	}
	return cases
}

func matchCases() []matchCase {
	requests := []email.RemovalRequest{{ID: 1, BrokerID: "broker-a"}, {ID: 2, BrokerID: "broker-b"}}
	messages := []email.Message{
		{ID: "1", Subject: "Re: Data Deletion Request — broker-a", MessageID: "<m1@example.com>", ThreadID: "<sent-1@example.com>", IMAPUID: 1},
		{ID: "2", Subject: "AW: Data Deletion Request — BROKER-B", MessageID: "<m2@example.com>", ThreadID: "<m2@example.com>", IMAPUID: 2},
		{ID: "3", Subject: "Unrelated", MessageID: "<m3@example.com>", ThreadID: "<sent-1@example.com>", IMAPUID: 3},
		{ID: "4", Subject: "Data Deletion Request — broker-a", ThreadID: "", IMAPUID: 4},
		{ID: "5", Subject: "Re: Data Deletion Request — broker-z", MessageID: "<m5@example.com>", ThreadID: "<m5@example.com>", IMAPUID: 5},
	}
	emptyThread := map[string]int64{"": 9}
	return []matchCase{
		{
			Name:      "thread_wins_over_subject",
			Messages:  messages,
			Requests:  requests,
			ThreadMap: map[string]int64{"<sent-1@example.com>": 2},
			Output:    mustMarshal(email.MatchReplyToRequest(messages, requests, map[string]int64{"<sent-1@example.com>": 2})),
		},
		{
			Name:      "an_empty_thread_key_never_matches",
			Messages:  messages,
			Requests:  requests,
			ThreadMap: emptyThread,
			Output:    mustMarshal(email.MatchReplyToRequest(messages, requests, emptyThread)),
		},
		{
			Name:     "no_requests_means_no_match",
			Messages: messages,
			Requests: nil,
			Output:   mustMarshal(email.MatchReplyToRequest(messages, nil, nil)),
		},
	}
}

// scriptedDialer hands out one scripted session per Dial call. Every search the
// policy performs is recorded in order, per folder.
type scriptedDialer struct {
	script   map[string]folderScript
	searches []searchWire
	fetches  [][]uint32
}

type scriptedSession struct {
	dialer *scriptedDialer
	folder string
	script folderScript
}

func (d *scriptedDialer) Dial(_ context.Context, cfg email.IMAPConfig) (email.IMAPSession, error) {
	script, ok := d.script[cfg.Folder]
	if !ok {
		return nil, fmt.Errorf("scripted dialer: no script for folder %q", cfg.Folder)
	}
	return &scriptedSession{dialer: d, folder: cfg.Folder, script: script}, nil
}

func (s *scriptedSession) Select(_ context.Context, _ string) (uint32, error) {
	if s.script.SelectError != "" {
		return 0, fmt.Errorf("%s", s.script.SelectError)
	}
	return s.script.UIDValidity, nil
}

func (s *scriptedSession) SearchUID(_ context.Context, uidRange string, since ...time.Time) ([]uint32, error) {
	s.dialer.searches = append(s.dialer.searches, searchWire{
		Folder:   s.folder,
		UIDRange: uidRange,
		SinceSet: len(since) > 0,
	})
	if s.script.SearchError != "" {
		return nil, fmt.Errorf("%s", s.script.SearchError)
	}
	return append([]uint32(nil), s.script.UIDs...), nil
}

func (s *scriptedSession) Fetch(_ context.Context, uids []uint32) ([]email.FetchedMessage, error) {
	s.dialer.fetches = append(s.dialer.fetches, append([]uint32(nil), uids...))
	if s.script.FetchError != "" {
		return nil, fmt.Errorf("%s", s.script.FetchError)
	}
	omitted := map[uint32]struct{}{}
	for _, uid := range s.script.Omit {
		omitted[uid] = struct{}{}
	}
	var out []email.FetchedMessage
	for _, fetched := range s.script.Fetched {
		if _, skip := omitted[fetched.UID]; skip {
			continue
		}
		message := email.FetchedMessage{
			UID:    fetched.UID,
			Header: []byte(fetched.Header),
			Body:   []byte(fetched.Body),
			Flags:  fetched.Flags,
		}
		if fetched.InternalDate != "" {
			parsed, err := time.Parse(time.RFC3339, fetched.InternalDate)
			if err != nil {
				fail(err)
			}
			message.InternalDate = parsed
		}
		out = append(out, message)
	}
	return out, nil
}

func (s *scriptedSession) Close() error { return nil }

// hwmSnapshot reads every folder a case touched, so a case can show both the
// folders that advanced and the folders that were left alone.
func hwmSnapshot(store email.HWMStore, host string, folders []string) []hwmWire {
	out := []hwmWire{}
	for _, folder := range folders {
		validity, last, err := store.Get(context.Background(), host, folder)
		if err != nil {
			fail(err)
		}
		if validity == nil && last == nil {
			continue
		}
		out = append(out, hwmWire{Host: host, Folder: folder, UIDValidity: validity, LastUID: last})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Folder < out[j].Folder })
	return out
}

func seedHWM(store email.HWMStore, entries []hwmWire) {
	for _, entry := range entries {
		if entry.UIDValidity == nil || entry.LastUID == nil {
			continue
		}
		if err := store.Set(context.Background(), entry.Host, entry.Folder, *entry.UIDValidity, *entry.LastUID); err != nil {
			fail(err)
		}
	}
}

// simpleHeader builds the raw RFC 5322 header block for one scripted message.
func simpleHeader(subject, messageID, references, date string) string {
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

func readHeaders() (inbox folderScript, replies folderScript) {
	header := simpleHeader
	inbox = folderScript{
		UIDValidity: 42,
		UIDs:        []uint32{6, 1, 2},
		Fetched: []fetchedWire{
			{UID: 1, Header: header("=?UTF-8?Q?Re=3A_Data_Deletion_Request_=E2=80=94_broker-a?=", "<m1@example.com>", "<sent@example.com>", "Mon, 21 Jul 2026 10:00:00 +0000"), Body: "  removed  ", Flags: []string{"\\Seen"}},
			{UID: 2, Header: header("Second", "<m2@example.com>", "", "Mon, 21 Jul 2026 11:00:00 +0000"), Body: "second"},
			{UID: 6, Header: header("Third", "<m6@example.com>", "", ""), Body: "third", InternalDate: "2026-07-22T09:30:00Z"},
		},
	}
	// Replies repeats UID 2's Message-ID, so the cross-folder dedupe has work.
	replies = folderScript{
		UIDValidity: 9,
		UIDs:        []uint32{4, 5},
		Fetched: []fetchedWire{
			{UID: 4, Header: header("Second", "<m2@example.com>", "", "Mon, 21 Jul 2026 11:00:00 +0000"), Body: "duplicate"},
			{UID: 5, Header: header("Re: Data Deletion Request — broker-a", "<m5@example.com>", "<sent@example.com>", "Mon, 21 Jul 2026 12:00:00 +0000"), Body: "reply"},
		},
	}
	return inbox, replies
}

func pollCases() []pollCase {
	inbox, replies := readHeaders()
	scripts := func(folders []string) map[string]folderScript {
		out := map[string]folderScript{}
		for _, folder := range folders {
			if folder == "INBOX" {
				out[folder] = inbox
			} else {
				out[folder] = replies
			}
		}
		return out
	}

	// run polls the configured folders `polls` times and records each answer
	// together with the searches the policy performed for it.
	runScript := func(name string, folders []string, initial []hwmWire, maxMessages, sinceDays, polls int, script map[string]folderScript) pollCase {
		if len(folders) == 0 {
			folders = []string{"INBOX"}
		}
		state := email.NewMemoryHWMStore()
		seedHWM(state, initial)
		calls := make([]callWire, 0, polls)
		for range polls {
			dialer := &scriptedDialer{script: script}
			cfg := email.IMAPConfig{Host: "imap.example.com", Folder: folders[0], MaxMessages: maxMessages, SinceDays: sinceDays}
			var messages []email.Message
			var err error
			if len(folders) > 1 {
				messages, err = email.PollFolders(context.Background(), cfg, folders, dialer, state)
			} else {
				messages, err = email.PollInbox(context.Background(), cfg, dialer, state)
			}
			call := callWire{Searches: dialer.searches}
			if call.Searches == nil {
				call.Searches = []searchWire{}
			}
			call.Fetches = dialer.fetches
			if call.Fetches == nil {
				call.Fetches = [][]uint32{}
			}
			if err != nil {
				call.Error = err.Error()
			} else {
				call.Messages = mustMarshal(messages)
			}
			calls = append(calls, call)
		}
		return pollCase{
			Name:      name,
			Mode:      mode(folders),
			Host:      "imap.example.com",
			Folder:    folders[0],
			Max:       maxMessages,
			SinceDays: sinceDays,
			Folders:   folders,
			Initial:   initialOrEmpty(initial),
			Script:    script,
			Polls:     polls,
			Calls:     calls,
			Final:     hwmSnapshot(state, "imap.example.com", folders),
		}
	}
	run := func(name string, folders []string, initial []hwmWire, maxMessages, sinceDays, polls int) pollCase {
		return runScript(name, folders, initial, maxMessages, sinceDays, polls, scripts(folders))
	}

	one := []string{"INBOX"}
	failing := func(script folderScript) map[string]folderScript {
		return map[string]folderScript{"INBOX": script}
	}
	return []pollCase{
		// A cold start searches from UID 1, consumes the oldest window first and
		// advances the HWM to the last UID it handled.
		run("cold_start_then_continue", one, nil, 0, 0, 2),
		// The stored HWM continues the search.
		run("stored_hwm_continues", one, []hwmWire{{Host: "imap.example.com", Folder: "INBOX", UIDValidity: u32(42), LastUID: u32(5)}}, 0, 0, 1),
		// A different UIDVALIDITY forces a cold start from 1.
		run("stale_uidvalidity_cold_starts", one, []hwmWire{{Host: "imap.example.com", Folder: "INBOX", UIDValidity: u32(41), LastUID: u32(5)}}, 0, 0, 1),
		// The oldest pending window is consumed first.
		run("oldest_window_first", one, nil, 2, 0, 3),
		// Search and fetch failures leave the HWM untouched.
		runScript("search_failure_keeps_the_hwm", one, nil, 0, 0, 1, failing(folderScript{UIDValidity: 42, SearchError: "mailbox unavailable"})),
		runScript("fetch_failure_keeps_the_hwm", one, nil, 0, 0, 1, failing(folderScript{UIDValidity: 42, UIDs: []uint32{1}, FetchError: "connection reset"})),
		// A FETCH answer that omits a requested UID is an error, not a silent skip.
		runScript("fetch_omission_is_an_error", one, nil, 0, 0, 1, failing(folderScript{UIDValidity: 42, UIDs: []uint32{1, 2}, Fetched: inbox.Fetched, Omit: []uint32{2}})),
		// Select failures fail before any UID work.
		runScript("select_failure", one, nil, 0, 0, 1, failing(folderScript{UIDValidity: 42, SelectError: "no such folder"})),
		// An empty mailbox still records the validated UIDVALIDITY.
		runScript("empty_mailbox_records_the_uidvalidity", one, nil, 0, 0, 1, failing(folderScript{UIDValidity: 42})),
		// An exhausted UID space is refused rather than wrapped.
		run("exhausted_uid_space", one, []hwmWire{{Host: "imap.example.com", Folder: "INBOX", UIDValidity: u32(42), LastUID: u32(4294967295)}}, 0, 0, 1),
		// The since window keeps what is at or after the cut and drops undated
		// messages. Both dates are extreme on purpose: the case must not move
		// with the wall clock.
		runScript("since_window_drops_undated", one, nil, 0, 7, 1, failing(folderScript{
			UIDValidity: 42,
			UIDs:        []uint32{1, 2, 3},
			Fetched: []fetchedWire{
				{UID: 1, Header: simpleHeader("Old", "<old@example.com>", "", "Thu, 01 Jan 1970 00:00:00 +0000"), Body: "old"},
				{UID: 2, Header: simpleHeader("Future", "<future@example.com>", "", "Fri, 01 Jan 2199 00:00:00 +0000"), Body: "future"},
				{UID: 3, Header: simpleHeader("Undated", "<undated@example.com>", "", ""), Body: "undated"},
			},
		})),
		// Cross-folder polling deduplicates by Message-ID.
		run("folders_deduplicate_by_message_id", []string{"INBOX", "Replies"}, nil, 0, 0, 1),
	}
}

func mode(folders []string) string {
	if len(folders) > 1 {
		return "folders"
	}
	return "inbox"
}

func initialOrEmpty(initial []hwmWire) []hwmWire {
	if initial == nil {
		return []hwmWire{}
	}
	return initial
}

// failingStore fails the first insert, so the staged HWM must not be committed.
type failingStore struct {
	inner email.ReplyStore
}

func (f *failingStore) Insert(context.Context, email.MatchedMessage, string) error {
	return fmt.Errorf("reply insert failed")
}

// recordingStore captures what the service persisted.
type recordingStore struct {
	inner   email.ReplyStore
	inserts []insertWire
}

func (r *recordingStore) Insert(_ context.Context, reply email.MatchedMessage, snippet string) error {
	r.inserts = append(r.inserts, insertWire{
		RequestID: reply.RequestID,
		MessageID: messageKey(reply.Message),
		ThreadID:  reply.Message.ThreadID,
		From:      reply.Message.From,
		Subject:   reply.Message.Subject,
		Snippet:   snippet,
	})
	return nil
}

func serviceCases() []serviceCase {
	inbox, replies := readHeaders()
	requests := []email.RemovalRequest{{ID: 7, BrokerID: "broker-a"}}
	threadMap := map[string]int64{"<sent@example.com>": 7}

	run := func(name string, folders []string, fail bool) serviceCase {
		if len(folders) == 0 {
			folders = []string{"INBOX"}
		}
		script := map[string]folderScript{"INBOX": inbox}
		if len(folders) > 1 {
			script["Replies"] = replies
		}
		state := email.NewMemoryHWMStore()
		dialer := &scriptedDialer{script: script}
		store := &recordingStore{}
		var replyStore email.ReplyStore = store
		if fail {
			replyStore = &failingStore{}
		}
		service := email.NewInboxService(dialer, state)
		matched, err := service.PollAndMatch(context.Background(), email.IMAPConfig{Host: "imap.example.com", Folder: folders[0]}, folders, requests, threadMap, replyStore)
		out := serviceCase{
			Name:      name,
			Host:      "imap.example.com",
			Folder:    folders[0],
			Folders:   folders,
			Requests:  requests,
			ThreadMap: threadMap,
			Initial:   []hwmWire{},
			Script:    script,
			Inserts:   store.inserts,
			Final:     hwmSnapshot(state, "imap.example.com", folders),
		}
		if err != nil {
			out.Error = err.Error()
		} else {
			out.Output = mustMarshal(matched)
		}
		if out.Inserts == nil {
			out.Inserts = []insertWire{}
		}
		return out
	}

	return []serviceCase{
		run("service_persists_then_commits", nil, false),
		run("service_keeps_the_hwm_when_the_insert_fails", nil, true),
		run("service_across_folders", []string{"INBOX", "Replies"}, false),
	}
}

func configCases() []imapConfigCase {
	inputs := []imapConfigCase{
		{Name: "defaults", Env: map[string]string{}},
		{
			Name: "explicit_values",
			Env: map[string]string{
				"IMAP_HOST":                "imap.example.com",
				"IMAP_PORT":                "143",
				"IMAP_USERNAME":            "me@example.com",
				"IMAP_PASSWORD":            "literal-secret",
				"IMAP_SSL":                 "0",
				"IMAP_FOLDER":              "Replies",
				"IMAP_SINCE_DAYS":          "3",
				"IMAP_MAX_MESSAGES":        "9",
				"IMAP_OAUTH2_USERNAME":     "oauth@example.com",
				"IMAP_OAUTH2_ACCESS_TOKEN": "literal-token",
			},
		},
		{
			Name:        "oauth2_arguments_win",
			Env:         map[string]string{"IMAP_USERNAME": "me@example.com", "IMAP_OAUTH2_ACCESS_TOKEN": "environment-token"},
			AccessToken: "argument-token",
			OAuthUser:   "override@example.com",
		},
		{Name: "invalid_port", Env: map[string]string{"IMAP_PORT": "99999"}},
		{Name: "invalid_port_text", Env: map[string]string{"IMAP_PORT": "imap"}},
		{
			Name: "odd_boolean_and_negative_numbers",
			Env: map[string]string{
				"IMAP_SSL":          "maybe",
				"IMAP_SINCE_DAYS":   "-4",
				"IMAP_MAX_MESSAGES": "zero",
				"IMAP_FOLDER":       "",
			},
		},
		{Name: "oauth2_username_falls_back_to_the_account", Env: map[string]string{"IMAP_USERNAME": "me@example.com", "IMAP_OAUTH2_ACCESS_TOKEN": "literal-token"}},
	}

	cases := make([]imapConfigCase, 0, len(inputs))
	for _, input := range inputs {
		restore := setEnv(input.Env)
		cfg, err := email.LoadIMAPConfigWithOptions(email.IMAPConfigOptions{
			OAuth2AccessToken: input.AccessToken,
			OAuth2Username:    input.OAuthUser,
		})
		restore()
		out := input
		if err != nil {
			out.Error = err.Error()
		} else {
			out.Config = mustMarshal(configWire(cfg))
		}
		cases = append(cases, out)
	}
	return cases
}

// configWire is the redacted JSON form of an IMAPConfig: neither the password
// nor the access token ever reaches a fixture, only whether one was resolved.
func configWire(cfg email.IMAPConfig) map[string]any {
	out := map[string]any{
		"host":         cfg.Host,
		"port":         cfg.Port,
		"username":     cfg.Username,
		"use_tls":      cfg.UseTLS,
		"folder":       cfg.Folder,
		"since_days":   cfg.SinceDays,
		"max_messages": cfg.MaxMessages,
		"password_set": cfg.Password != "",
		"oauth2":       nil,
	}
	if cfg.OAuth2 != nil {
		out["oauth2"] = map[string]any{
			"username":         cfg.OAuth2.Username,
			"access_token_set": cfg.OAuth2.AccessToken != "",
		}
	}
	return out
}

// setEnv applies a fixture environment and returns the restore function. Only
// IMAP_* keys are touched; every key the fixture does not set is cleared so a
// developer's shell cannot leak into the measurement.
func setEnv(values map[string]string) func() {
	keys := []string{
		"IMAP_HOST", "IMAP_PORT", "IMAP_USERNAME", "IMAP_PASSWORD", "IMAP_SSL",
		"IMAP_FOLDER", "IMAP_SINCE_DAYS", "IMAP_MAX_MESSAGES",
		"IMAP_OAUTH2_USERNAME", "IMAP_OAUTH2_ACCESS_TOKEN",
	}
	previous := map[string]*string{}
	for _, key := range keys {
		if value, ok := os.LookupEnv(key); ok {
			previous[key] = &value
		} else {
			previous[key] = nil
		}
		if value, ok := values[key]; ok {
			if err := os.Setenv(key, value); err != nil {
				fail(err)
			}
			continue
		}
		if err := os.Unsetenv(key); err != nil {
			fail(err)
		}
	}
	return func() {
		for key, value := range previous {
			if value == nil {
				if err := os.Unsetenv(key); err != nil {
					fail(err)
				}
				continue
			}
			if err := os.Setenv(key, *value); err != nil {
				fail(err)
			}
		}
	}
}

func messageKey(message email.Message) string {
	if message.MessageID != "" {
		return message.MessageID
	}
	return message.ID
}

func mustMarshal(value any) string {
	encoded, err := json.Marshal(value)
	if err != nil {
		fail(err)
	}
	return string(encoded)
}

func u32(value uint32) *uint32 { return &value }

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
