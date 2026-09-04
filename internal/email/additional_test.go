package email

import (
	"context"
	"errors"
	"net"
	"strings"
	"testing"
	"time"
)

func testTime() time.Time {
	return time.Date(2026, 8, 31, 12, 0, 0, 0, time.UTC)
}

type failingReader struct{}

func (failingReader) Read([]byte) (int, error) { return 0, errors.New("read failed") }

func TestReadBoundedBodyPropagatesErrorsAndRejectsTruncation(t *testing.T) {
	if _, err := readBoundedBody(failingReader{}); err == nil || !strings.Contains(err.Error(), "read failed") {
		t.Fatalf("read error = %v", err)
	}
	if _, err := readBoundedBody(strings.NewReader(strings.Repeat("x", 64*1024+1))); err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("oversized body error = %v", err)
	}
}

func TestSetContextDeadlineInterruptsOnCancellation(t *testing.T) {
	client, server := net.Pipe()
	defer client.Close()
	defer server.Close()
	ctx, cancel := context.WithCancel(context.Background())
	cleanup := setContextDeadline(ctx, client, time.Hour)
	cancel()
	started := time.Now()
	_, err := client.Read(make([]byte, 1))
	cleanup()
	if err == nil || time.Since(started) > time.Second {
		t.Fatalf("cancellation did not interrupt read promptly: err=%v elapsed=%s", err, time.Since(started))
	}
}

func TestBuildMIMEValidatesAndSanitizesHeaders(t *testing.T) {
	if _, _, err := BuildMIMEAt(EmailMessage{To: "to@example.test"}, "", testTime(), "<id@example.test>"); !errors.Is(err, ErrSMTPFromMissing) {
		t.Fatalf("missing sender error = %v", err)
	}
	if _, _, err := BuildMIMEAt(EmailMessage{}, "from@example.test", testTime(), "<id@example.test>"); err == nil {
		t.Fatal("missing recipient accepted")
	}
	raw, _, err := BuildMIMEAt(EmailMessage{To: "to@example.test\r\nBcc: leaked", Subject: "subject\nInjected", Body: "line\r\nnext"}, "from@example.test\nInjected", testTime(), "")
	if err != nil {
		t.Fatal(err)
	}
	text := string(raw)
	if strings.Contains(text, "\r\nInjected") || strings.Contains(text, "\r\nBcc: leaked") || !strings.Contains(text, "line\nnext") {
		t.Fatalf("header/body sanitization failed: %q", text)
	}
}

func TestRecipientsAndBatchSendPreserveOneResultPerMessage(t *testing.T) {
	message := EmailMessage{To: " a@example.test, ,b@example.test ", CC: "c@example.test", BCC: "d@example.test"}
	got := Recipients(message)
	if strings.Join(got, ",") != "a@example.test,b@example.test,c@example.test,d@example.test" {
		t.Fatalf("recipients = %#v", got)
	}
	recorder := &recordingSMTP{}
	results := SendMessagesBatch(context.Background(), SMTPConfig{From: "from@example.test"}, []EmailMessage{
		{To: "ok@example.test", Subject: "ok", Body: "body"},
		{To: "", Subject: "bad"},
	}, recorder)
	if len(results) != 2 || !results[0].Success || results[1].Success || results[1].Error == "" {
		t.Fatalf("batch results = %#v", results)
	}
}

func TestSMTPValidationAndOAuthChallenge(t *testing.T) {
	transport := NetSMTPTransport{}
	if err := transport.Send(context.Background(), SMTPConfig{}, nil, nil); err == nil {
		t.Fatal("empty SMTP request accepted")
	}
	if err := transport.Send(context.Background(), SMTPConfig{Host: "smtp.example.test"}, []string{"to@example.test"}, nil); err == nil {
		t.Fatal("invalid SMTP port accepted")
	}
	auth := xoauth2Auth{token: OAuth2Token{Username: "u@example.test", AccessToken: "access"}}
	mechanism, initial, err := auth.Start(nil)
	if err != nil || mechanism != "XOAUTH2" || !strings.Contains(string(initial), "Bearer access") {
		t.Fatalf("oauth start = %q %q err=%v", mechanism, initial, err)
	}
	if _, err := auth.Next(nil, true); err == nil {
		t.Fatal("OAuth challenge was accepted")
	}
	if next, err := auth.Next(nil, false); err != nil || next != nil {
		t.Fatalf("OAuth final response = %v, err=%v", next, err)
	}
	if _, _, err := (xoauth2Auth{}).Start(nil); err == nil {
		t.Fatal("empty OAuth credentials accepted")
	}
}

func TestParseFetchedMessageFallbacksAndSubjectBounds(t *testing.T) {
	message, err := ParseFetchedMessage(FetchedMessage{
		UID:    7,
		Header: []byte("Subject: Re: Data\r\nFrom: Broker <b@example.test>\r\nTo: me@example.test\r\nDate: not-a-date\r\nIn-Reply-To: <thread@example.test>\r\nMessage-ID: <reply@example.test>\r\n\r\n"),
		Body:   []byte(" body "), Flags: []string{"Seen"},
	})
	if err != nil || message.ID != "7" || message.ThreadID != "<thread@example.test>" || message.Date != nil || message.Body != " body " {
		t.Fatalf("parsed message = %#v, err=%v", message, err)
	}
	if !SubjectMatches("Data", "Aw: Re: Data") || SubjectMatches("Data", "Other") {
		t.Fatal("subject normalization mismatch")
	}
	if ParseEmailBody("  abc  ", 0) != "abc" || ParseEmailBody("abcdef", 3) != "abc..." {
		t.Fatal("email body bounds mismatch")
	}
	fallback, fallbackErr := ParseFetchedMessage(FetchedMessage{
		UID:    8,
		Header: []byte("Subject: =?UTF-8?Q?Antwort?=\r\nFrom: broker@example.test\r\nTo: me@example.test\r\nDate: Mon, 21 Jul 2026 10:00:00 +0000\r\nMessage-ID: <fallback@example.test>\r\n\r\n"),
	})
	if fallbackErr != nil || fallback.ThreadID != "<fallback@example.test>" || fallback.Date == nil || fallback.Subject != "Antwort" {
		t.Fatalf("message-id/date fallback = %#v, err=%v", fallback, fallbackErr)
	}
}

func TestPollInboxConfigurationAndSearchErrors(t *testing.T) {
	ctx := context.Background()
	if _, err := PollInbox(ctx, IMAPConfig{}, nil, nil); err == nil {
		t.Fatal("nil IMAP dialer accepted")
	}
	if _, err := PollInbox(ctx, IMAPConfig{Host: "imap.example.test"}, failingDialer{err: errors.New("dial")}, nil); err == nil || !strings.Contains(err.Error(), "connect/login") {
		t.Fatalf("dial error = %v", err)
	}
	session := &configurableSession{uidValidity: 1, searchErr: errors.New("search")}
	if _, err := PollInbox(ctx, IMAPConfig{Host: "imap.example.test"}, configurableDialer{session}, nil); err == nil || !strings.Contains(err.Error(), "UID search") {
		t.Fatalf("search error = %v", err)
	}
	if session.selectedFolder != "INBOX" {
		t.Fatalf("default folder = %q", session.selectedFolder)
	}
	if _, err := PollInbox(ctx, IMAPConfig{Host: ""}, configurableDialer{&configurableSession{}}, nil); err == nil {
		t.Fatal("empty IMAP host accepted")
	}
	selectFailed := &configurableSession{uidValidity: 1, selectErr: errors.New("select")}
	if _, err := PollInbox(ctx, IMAPConfig{Host: "imap.example.test"}, configurableDialer{selectFailed}, nil); err == nil || !strings.Contains(err.Error(), "folder select") {
		t.Fatalf("select error = %v", err)
	}
	if _, err := PollInbox(ctx, IMAPConfig{Host: "imap.example.test"}, configurableDialer{&configurableSession{uidValidity: 1}}, failingHWM{getErr: errors.New("read hwm")}); err == nil || !strings.Contains(err.Error(), "high-water mark") {
		t.Fatalf("HWM read error = %v", err)
	}
	if _, err := PollInbox(ctx, IMAPConfig{Host: "imap.example.test"}, configurableDialer{&configurableSession{uidValidity: 1}}, failingHWM{setErr: errors.New("write hwm")}); err == nil || !strings.Contains(err.Error(), "write hwm") {
		t.Fatalf("HWM write error = %v", err)
	}
}

type failingDialer struct{ err error }

func (d failingDialer) Dial(context.Context, IMAPConfig) (IMAPSession, error) { return nil, d.err }

type configurableDialer struct{ session IMAPSession }

func (d configurableDialer) Dial(context.Context, IMAPConfig) (IMAPSession, error) {
	return d.session, nil
}

type failingHWM struct {
	getErr error
	setErr error
}

func (f failingHWM) Get(context.Context, string, string) (*uint32, *uint32, error) {
	return nil, nil, f.getErr
}
func (f failingHWM) Set(context.Context, string, string, uint32, uint32) error {
	return f.setErr
}

type configurableSession struct {
	uidValidity    uint32
	selectedFolder string
	selectErr      error
	searchErr      error
}

func (s *configurableSession) Select(_ context.Context, folder string) (uint32, error) {
	s.selectedFolder = folder
	return s.uidValidity, s.selectErr
}
func (s *configurableSession) SearchUID(context.Context, string, ...time.Time) ([]uint32, error) {
	return nil, s.searchErr
}
func (s *configurableSession) Fetch(context.Context, []uint32) ([]FetchedMessage, error) {
	return nil, nil
}
func (s *configurableSession) Close() error { return nil }
