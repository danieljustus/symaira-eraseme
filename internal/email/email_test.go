package email

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"
)

type recordingSMTP struct {
	cfg        SMTPConfig
	recipients []string
	message    []byte
	err        error
}

func (r *recordingSMTP) Send(_ context.Context, cfg SMTPConfig, recipients []string, message []byte) error {
	r.cfg = cfg
	r.recipients = append([]string(nil), recipients...)
	r.message = append([]byte(nil), message...)
	return r.err
}

func TestBuildMIMEAndSendKeepsBCCOutOfHeaders(t *testing.T) {
	msg := EmailMessage{To: "to@example.com", CC: "cc@example.com", BCC: "secret@example.com", Subject: "Data request", Body: "Hello"}
	when := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	raw, id, err := BuildMIMEAt(msg, "me@example.com", when, "<fixed@example.com>")
	if err != nil {
		t.Fatal(err)
	}
	text := string(raw)
	for _, want := range []string{"From: me@example.com", "To: to@example.com", "Cc: cc@example.com", "Subject: Data request", "Message-ID: <fixed@example.com>", "Hello"} {
		if !strings.Contains(text, want) {
			t.Errorf("MIME missing %q", want)
		}
	}
	if strings.Contains(text, "secret@example.com") {
		t.Fatal("BCC leaked into MIME headers")
	}
	if id != "<fixed@example.com>" {
		t.Fatalf("message id = %q", id)
	}

	recorder := &recordingSMTP{}
	gotID, err := SendMessage(context.Background(), SMTPConfig{From: "me@example.com"}, msg, recorder)
	if err != nil {
		t.Fatal(err)
	}
	if gotID == "" || len(recorder.message) == 0 {
		t.Fatal("message was not generated")
	}
	if strings.Join(recorder.recipients, ",") != "to@example.com,cc@example.com,secret@example.com" {
		t.Fatalf("recipients = %#v", recorder.recipients)
	}
}

func TestLoadConfigAndSafeSecretFailure(t *testing.T) {
	t.Setenv("SYMERASEME_SMTP_HOST", "smtp.example.com")
	t.Setenv("SYMERASEME_SMTP_PORT", "2525")
	t.Setenv("SYMERASEME_SMTP_TLS", "false")
	t.Setenv("SYMERASEME_SMTP_FROM", "me@example.com")
	cfg, err := LoadSMTPConfig()
	if err != nil || cfg.Host != "smtp.example.com" || cfg.Port != 2525 || cfg.UseTLS {
		t.Fatalf("SMTP config = %#v, err=%v", cfg, err)
	}
	t.Setenv("IMAP_PASSWORD", "symvault://UNIQUE_SECRET_VALUE")
	_, err = LoadIMAPConfig()
	if err == nil || strings.Contains(err.Error(), "UNIQUE_SECRET_VALUE") {
		t.Fatalf("secret leaked or error missing: %v", err)
	}
}

type fakeSession struct {
	uidValidity uint32
	uids        []uint32
	fetched     []FetchedMessage
	searches    []string
	fetches     [][]uint32
	searchErr   error
	fetchErr    error
}

func (s *fakeSession) Select(context.Context, string) (uint32, error) { return s.uidValidity, nil }
func (s *fakeSession) SearchUID(_ context.Context, query string) ([]uint32, error) {
	s.searches = append(s.searches, query)
	return append([]uint32(nil), s.uids...), s.searchErr
}
func (s *fakeSession) Fetch(_ context.Context, uids []uint32) ([]FetchedMessage, error) {
	s.fetches = append(s.fetches, append([]uint32(nil), uids...))
	return append([]FetchedMessage(nil), s.fetched...), s.fetchErr
}
func (s *fakeSession) Close() error { return nil }

type fakeDialer struct{ session *fakeSession }

func (d fakeDialer) Dial(context.Context, IMAPConfig) (IMAPSession, error) { return d.session, nil }

func TestPollInboxUsesUIDHWMAndParsesHeaders(t *testing.T) {
	session := &fakeSession{
		uidValidity: 42,
		uids:        []uint32{1, 2, 6},
		fetched:     []FetchedMessage{{UID: 6, Header: []byte("Subject: =?UTF-8?Q?Re=3A_Data?=\r\nFrom: Broker <b@example.com>\r\nTo: me@example.com\r\nDate: Mon, 21 Jul 2026 10:00:00 +0000\r\nMessage-ID: <reply@example.com>\r\nReferences: <sent@example.com>\r\n\r\n"), Body: []byte("removed")}},
	}
	state := NewMemoryHWMStore()
	cfg := IMAPConfig{Host: "imap.example.com", Folder: "INBOX", MaxMessages: 2}
	messages, err := PollInbox(context.Background(), cfg, fakeDialer{session}, state)
	if err != nil {
		t.Fatal(err)
	}
	if len(messages) != 1 || messages[0].Subject != "Re: Data" || messages[0].ThreadID != "<sent@example.com>" || messages[0].Body != "removed" {
		t.Fatalf("messages = %#v", messages)
	}
	if session.searches[0] != "1:*" || len(session.fetches[0]) != 2 || session.fetches[0][0] != 2 || session.fetches[0][1] != 6 {
		t.Fatalf("search/fetch = %#v / %#v", session.searches, session.fetches)
	}
	_, last, err := state.Get(context.Background(), cfg.Host, cfg.Folder)
	if err != nil || last == nil || *last != 6 {
		t.Fatalf("HWM = %v, err=%v", last, err)
	}

	session.uids = nil
	_, err = PollInbox(context.Background(), cfg, fakeDialer{session}, state)
	if err != nil {
		t.Fatal(err)
	}
	if session.searches[1] != "7:*" {
		t.Fatalf("second search = %q", session.searches[1])
	}
}

func TestPollInboxUIDValidityMismatchColdStartsAndFetchErrorAdvances(t *testing.T) {
	state := NewMemoryHWMStore()
	_ = state.Set(context.Background(), "imap.example.com", "INBOX", 1, 99)
	session := &fakeSession{uidValidity: 2, uids: []uint32{3, 4}, fetchErr: errTest}
	_, err := PollInbox(context.Background(), IMAPConfig{Host: "imap.example.com", Folder: "INBOX"}, fakeDialer{session}, state)
	if err == nil || !strings.Contains(err.Error(), "UID fetch") {
		t.Fatalf("fetch error = %v", err)
	}
	if session.searches[0] != "1:*" {
		t.Fatalf("cold-start search = %q", session.searches[0])
	}
	validity, last, _ := state.Get(context.Background(), "imap.example.com", "INBOX")
	if validity == nil || last == nil || *validity != 2 || *last != 4 {
		t.Fatalf("HWM after failed fetch = %v/%v", validity, last)
	}
}

var errTest = &testError{}

type testError struct{}

func (*testError) Error() string { return "transport failed" }

func TestMatchReplyToRequestPrefersThread(t *testing.T) {
	messages := []Message{{Subject: "Re: Data Deletion Request — Other", ThreadID: "<sent@example.com>"}, {Subject: "Re: Data Deletion Request — Other"}}
	matched := MatchReplyToRequest(messages, []RemovalRequest{{ID: 1, BrokerID: "Other"}, {ID: 2, BrokerID: "Other"}}, map[string]int64{"<sent@example.com>": 9})
	if matched[0].RequestID == nil || *matched[0].RequestID != 9 || matched[0].MatchMethod != "thread" {
		t.Fatalf("thread match = %#v", matched[0])
	}
	if matched[1].RequestID == nil || matched[1].MatchMethod != "subject" {
		t.Fatalf("subject match = %#v", matched[1])
	}
}

func TestOAuthAuthorizeStateAndTokenExchange(t *testing.T) {
	stateFile := t.TempDir() + "/oauth2_state.json"
	clock := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	states := NewOAuthStateStore(stateFile)
	states.Now = func() time.Time { return clock }
	client := &OAuth2Client{States: states, Now: func() time.Time { return clock }}
	authURL, verifier, err := client.AuthorizeURL("gmail", "client", "http://127.0.0.1/callback")
	if err != nil {
		t.Fatal(err)
	}
	parsed, _ := url.Parse(authURL)
	query := parsed.Query()
	if query.Get("code_challenge_method") != "S256" || query.Get("state") == "" || verifier == "" {
		t.Fatalf("authorization query = %#v", query)
	}
	if err := states.Validate(query.Get("state")); err != nil {
		t.Fatal(err)
	}
	if err := states.Validate(query.Get("state")); err == nil {
		t.Fatal("OAuth state was not single-use")
	}
	info, err := os.Stat(stateFile)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("state file mode = %v, err=%v", info.Mode().Perm(), err)
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil || r.Form.Get("code_verifier") != "verifier" {
			t.Fatalf("token form = %v", r.Form)
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"access_token": "token", "refresh_token": "refresh"})
	}))
	defer server.Close()
	ProviderConfigs["test"] = ProviderConfig{TokenURL: server.URL}
	defer delete(ProviderConfigs, "test")
	result, err := client.ExchangeCode(context.Background(), "test", "code", "id", "secret", "http://callback", "verifier")
	if err != nil || result["access_token"] != "token" {
		t.Fatalf("token exchange = %#v, err=%v", result, err)
	}
}
