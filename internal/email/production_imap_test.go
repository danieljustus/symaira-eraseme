package email_test

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func TestProductionDialerLoginSelectSearchFetch(t *testing.T) {
	server, err := startFakeIMAPServer(true)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "bob"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "secret456"
	server.mu.Unlock()

	msgHeader := "From: broker@example.com\r\nTo: bob@example.com\r\nSubject: =?UTF-8?Q?Data_Request?=\r\nDate: Mon, 21 Jul 2026 10:00:00 +0000\r\nMessage-ID: <msg1@example.com>\r\n\r\n"
	msgBody := "Your data has been removed."
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 42,
		Messages: []fakeMessage{
			{UID: 10, Flags: []string{"\\Seen"}, Header: msgHeader, Body: msgBody},
		},
	}
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:        server.addr,
		Port:        server.port,
		Username:    "bob",
		Password:    "secret456",
		UseTLS:      true,
		Folder:      "INBOX",
		TLSConfig:   server.clientTLS,
		MaxMessages: 10,
	}

	dialer := email.NewNetIMAPDialer()
	state := email.NewMemoryHWMStore()

	messages, err := email.PollInbox(context.Background(), cfg, dialer, state)
	if err != nil {
		t.Fatalf("PollInbox failed: %v", err)
	}

	if len(messages) != 1 {
		t.Fatalf("messages len = %d, want 1", len(messages))
	}
	if messages[0].Subject != "Data Request" || messages[0].MessageID != "<msg1@example.com>" || messages[0].Body != msgBody {
		t.Fatalf("unexpected message: %#v", messages[0])
	}

	// Verify HWM updated
	v, u, err := state.Get(context.Background(), cfg.Host, "INBOX")
	if err != nil || v == nil || *v != 42 || u == nil || *u != 10 {
		t.Fatalf("unexpected HWM state: v=%v, u=%v, err=%v", v, u, err)
	}

	// Verify transcript includes expected commands
	transcript := server.getTranscript()
	joined := strings.Join(transcript, "\n")
	for _, expected := range []string{"LOGIN", "EXAMINE INBOX", "UID SEARCH", "UID FETCH", "LOGOUT"} {
		if !strings.Contains(joined, expected) {
			t.Errorf("transcript missing %q. Got:\n%s", expected, joined)
		}
	}
}

func TestProductionDialerSTARTTLSVerifiesCertificate(t *testing.T) {
	server, err := startFakeIMAPStartTLSServer()
	if err != nil {
		t.Fatalf("startFakeIMAPStartTLSServer: %v", err)
	}
	defer server.close()
	server.mu.Lock()
	server.requireUser = "user"
	server.requirePass = "pass"
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:     server.addr,
		Port:     server.port,
		Username: "user",
		Password: "pass",
		UseTLS:   false,
	}
	dialer := email.NewNetIMAPDialer()
	if _, err := dialer.Dial(context.Background(), cfg); err == nil {
		t.Fatal("STARTTLS accepted an untrusted certificate")
	}

	cfg.TLSConfig = server.clientTLS
	session, err := dialer.Dial(context.Background(), cfg)
	if err != nil {
		t.Fatalf("verified STARTTLS dial failed: %v", err)
	}
	if err := session.Close(); err != nil {
		t.Fatalf("close STARTTLS session: %v", err)
	}
	joined := strings.Join(server.getTranscript(), "\n")
	startTLS := strings.Index(joined, "STARTTLS")
	login := strings.LastIndex(joined, "LOGIN")
	if startTLS < 0 || login < 0 || startTLS > login {
		t.Fatalf("STARTTLS did not precede authentication: %s", joined)
	}
}

func TestProductionDialerXOAUTH2(t *testing.T) {
	server, err := startFakeIMAPServer(true)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireTok = "oauth_token_123"
	server.mu.Unlock()
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 77,
		Messages: []fakeMessage{
			{UID: 1, Header: "Subject: OAuth Msg\r\nMessage-ID: <m@test>\r\n\r\n", Body: "ok"},
		},
	}
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:      server.addr,
		Port:      server.port,
		Username:  "oauthuser@example.com",
		UseTLS:    true,
		TLSConfig: server.clientTLS,
		OAuth2: &email.OAuth2Token{
			Username:    "oauthuser@example.com",
			AccessToken: "oauth_token_123",
		},
	}

	dialer := email.NewNetIMAPDialer()
	messages, err := email.PollInbox(context.Background(), cfg, dialer, email.NewMemoryHWMStore())
	if err != nil {
		t.Fatalf("PollInbox with XOAUTH2 failed: %v", err)
	}
	if len(messages) != 1 || messages[0].Subject != "OAuth Msg" {
		t.Fatalf("unexpected messages: %#v", messages)
	}
}

func TestProductionDialerRedactedFailures(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	secretPass := "SUPER_SECRET_PASSWORD_VAL"
	server.mu.Lock()
	server.authErr = errors.New("bad password credential: " + secretPass)
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "user",
		Password:                   secretPass,
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
	}

	dialer := email.NewNetIMAPDialer()
	_, err = email.PollInbox(context.Background(), cfg, dialer, email.NewMemoryHWMStore())
	if err == nil {
		t.Fatal("expected error on failed login, got nil")
	}

	if strings.Contains(err.Error(), secretPass) {
		t.Fatalf("secret leaked in error message: %v", err)
	}
	if !strings.Contains(err.Error(), "[REDACTED]") && !strings.Contains(err.Error(), "connect/login failed") {
		t.Fatalf("unexpected error message: %v", err)
	}
}

func TestUIDValidityResetTriggersColdStart(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "user"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "pass"
	server.mu.Unlock()

	// First state: UIDVALIDITY 100, messages 1, 2
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 100,
		Messages: []fakeMessage{
			{UID: 1, Header: "Subject: msg1\r\n\r\n", Body: "1"},
			{UID: 2, Header: "Subject: msg2\r\n\r\n", Body: "2"},
		},
	}
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "user",
		Password:                   "pass",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
		Folder:                     "INBOX",
	}

	dialer := email.NewNetIMAPDialer()
	state := email.NewMemoryHWMStore()

	// Initial poll
	msgs1, err := email.PollInbox(context.Background(), cfg, dialer, state)
	if err != nil {
		t.Fatalf("Poll 1 failed: %v", err)
	}
	if len(msgs1) != 2 {
		t.Fatalf("expected 2 messages, got %d", len(msgs1))
	}

	v, u, _ := state.Get(context.Background(), cfg.Host, "INBOX")
	if *v != 100 || *u != 2 {
		t.Fatalf("expected HWM (100, 2), got (%d, %d)", *v, *u)
	}

	// Server rebuilds folder with new UIDVALIDITY 200 and messages UID 1
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 200,
		Messages: []fakeMessage{
			{UID: 1, Header: "Subject: reset msg\r\n\r\n", Body: "reset"},
		},
	}
	server.mu.Unlock()

	// Second poll: should detect mismatch and cold-start from 1:*
	msgs2, err := email.PollInbox(context.Background(), cfg, dialer, state)
	if err != nil {
		t.Fatalf("Poll 2 failed: %v", err)
	}
	if len(msgs2) != 1 || msgs2[0].Subject != "reset msg" {
		t.Fatalf("expected 1 cold-started message, got %#v", msgs2)
	}

	v2, u2, _ := state.Get(context.Background(), cfg.Host, "INBOX")
	if *v2 != 200 || *u2 != 1 {
		t.Fatalf("expected HWM updated to (200, 1), got (%d, %d)", *v2, *u2)
	}
}

func TestFetchFailureDoesNotAdvanceHWM(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "user"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "pass"
	server.mu.Unlock()
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 50,
		Messages: []fakeMessage{
			{UID: 10, Header: "Subject: msg\r\n\r\n"},
			{UID: 20, Header: "Subject: msg2\r\n\r\n"},
		},
	}
	server.mu.Unlock()
	server.mu.Lock()
	server.fetchErr = errors.New("simulated fetch failure")
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "user",
		Password:                   "pass",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
	}

	state := email.NewMemoryHWMStore()
	dialer := email.NewNetIMAPDialer()

	_, err = email.PollInbox(context.Background(), cfg, dialer, state)
	if err == nil || !strings.Contains(err.Error(), "UID fetch") {
		t.Fatalf("expected UID fetch error, got %v", err)
	}

	// A failed fetch must leave the HWM empty so the batch is retried.
	v, u, _ := state.Get(context.Background(), cfg.Host, "INBOX")
	if v != nil || u != nil {
		t.Fatalf("HWM advanced after failed fetch: v=%v, u=%v", v, u)
	}
}

func TestMultipleFoldersAndMessageIDDedup(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "user"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "pass"
	server.mu.Unlock()

	// Same message in INBOX and Archive with identical Message-ID
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 10,
		Messages: []fakeMessage{
			{UID: 1, Header: "Subject: Deletion\r\nMessage-ID: <shared@example.com>\r\n\r\n", Body: "copy 1"},
		},
	}
	server.mu.Unlock()
	server.mu.Lock()
	server.folders["Archive"] = &fakeFolder{
		UIDValidity: 20,
		Messages: []fakeMessage{
			{UID: 5, Header: "Subject: Deletion\r\nMessage-ID: <shared@example.com>\r\n\r\n", Body: "copy 2"},
			{UID: 6, Header: "Subject: Unique\r\nMessage-ID: <unique@example.com>\r\n\r\n", Body: "unique"},
		},
	}
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "user",
		Password:                   "pass",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
	}

	dialer := email.NewNetIMAPDialer()
	state := email.NewMemoryHWMStore()

	messages, err := email.PollFolders(context.Background(), cfg, []string{"INBOX", "Archive"}, dialer, state)
	if err != nil {
		t.Fatalf("PollFolders failed: %v", err)
	}

	// Should deduplicate <shared@example.com> across folders
	if len(messages) != 2 {
		t.Fatalf("expected 2 deduplicated messages, got %d: %#v", len(messages), messages)
	}
	if messages[0].MessageID != "<shared@example.com>" || messages[1].MessageID != "<unique@example.com>" {
		t.Fatalf("unexpected messages order/dedup: %#v", messages)
	}

	// Verify both folder HWMs persisted
	vInbox, uInbox, _ := state.Get(context.Background(), cfg.Host, "INBOX")
	vArchive, uArchive, _ := state.Get(context.Background(), cfg.Host, "Archive")
	if *vInbox != 10 || *uInbox != 1 {
		t.Fatalf("unexpected INBOX HWM: %v, %v", vInbox, uInbox)
	}
	if *vArchive != 20 || *uArchive != 20 && *uArchive != 6 {
		t.Fatalf("unexpected Archive HWM: %v, %v", vArchive, uArchive)
	}
}

func TestPersistenceAcrossProcessInstancesWithFakeServer(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "u"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "p"
	server.mu.Unlock()
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 99,
		Messages: []fakeMessage{
			{UID: 1, Header: "Subject: 1\r\n\r\n"},
			{UID: 2, Header: "Subject: 2\r\n\r\n"},
			{UID: 3, Header: "Subject: 3\r\n\r\n"},
		},
	}
	server.mu.Unlock()

	dbPath := filepath.Join(t.TempDir(), "symeraseme.db")

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "u",
		Password:                   "p",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
	}

	// Process 1 runs
	{
		store1, err := eventstore.Open(dbPath)
		if err != nil {
			t.Fatalf("Open store1: %v", err)
		}
		hwm1 := store1
		dialer1 := email.NewNetIMAPDialer()

		msgs, err := email.PollInbox(context.Background(), cfg, dialer1, hwm1)
		if err != nil || len(msgs) != 3 {
			t.Fatalf("Poll process 1 failed: %v, len=%d", err, len(msgs))
		}
		_ = store1.Close()
	}

	// New message arrives on server
	server.mu.Lock()
	server.folders["INBOX"].Messages = append(server.folders["INBOX"].Messages, fakeMessage{UID: 4, Header: "Subject: 4\r\n\r\n"})
	server.mu.Unlock()

	// Process 2 runs fresh
	{
		store2, err := eventstore.Open(dbPath)
		if err != nil {
			t.Fatalf("Open store2: %v", err)
		}
		defer store2.Close()
		hwm2 := store2
		dialer2 := email.NewNetIMAPDialer()

		msgs, err := email.PollInbox(context.Background(), cfg, dialer2, hwm2)
		if err != nil {
			t.Fatalf("Poll process 2 failed: %v", err)
		}
		// Should only fetch the new message (UID 4)
		if len(msgs) != 1 || msgs[0].Subject != "4" {
			t.Fatalf("process 2 expected 1 new message, got %d: %#v", len(msgs), msgs)
		}
	}
}

func TestTimeoutAndCancellation(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.delay = 500 * time.Millisecond
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "user",
		Password:                   "pass",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
		Timeout:                    50 * time.Millisecond,
	}

	dialer := email.NewNetIMAPDialer()
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()

	_, err = email.PollInbox(ctx, cfg, dialer, email.NewMemoryHWMStore())
	if err == nil {
		t.Fatal("expected timeout/cancellation error, got nil")
	}
}

func TestMalformedMessageDoesNotAdvanceHWM(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "u"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "p"
	server.mu.Unlock()
	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 1,
		Messages: []fakeMessage{
			{UID: 1, Invalid: true},
			{UID: 2, Header: "Subject: Valid\r\n\r\n", Body: "ok"},
		},
	}
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "u",
		Password:                   "p",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
	}

	dialer := email.NewNetIMAPDialer()
	state := email.NewMemoryHWMStore()
	_, err = email.PollInbox(context.Background(), cfg, dialer, state)
	if err == nil || !strings.Contains(err.Error(), "parse fetched message") {
		t.Fatalf("expected explicit parse failure, got %v", err)
	}
	validity, lastUID, getErr := state.Get(context.Background(), cfg.Host, cfg.Folder)
	if getErr != nil || validity != nil || lastUID != nil {
		t.Fatalf("malformed message advanced HWM: validity=%v uid=%v err=%v", validity, lastUID, getErr)
	}
}

func TestSinceDaysFiltering(t *testing.T) {
	server, err := startFakeIMAPServer(false)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	server.mu.Lock()
	server.requireUser = "u"
	server.mu.Unlock()
	server.mu.Lock()
	server.requirePass = "p"
	server.mu.Unlock()

	oldDate := time.Now().AddDate(0, 0, -10).Format(time.RFC1123Z)
	recentDate := time.Now().Add(-1 * time.Hour).Format(time.RFC1123Z)

	server.mu.Lock()
	server.folders["INBOX"] = &fakeFolder{
		UIDValidity: 1,
		Messages: []fakeMessage{
			{UID: 1, Header: "Subject: Old\r\nDate: " + oldDate + "\r\n\r\n", Body: "old"},
			{UID: 2, Header: "Subject: Recent\r\nDate: " + recentDate + "\r\n\r\n", Body: "recent"},
		},
	}
	server.mu.Unlock()

	cfg := email.IMAPConfig{
		Host:                       server.addr,
		Port:                       server.port,
		Username:                   "u",
		Password:                   "p",
		UseTLS:                     false,
		AllowInsecureCleartextAuth: true,
		SinceDays:                  2,
	}

	dialer := email.NewNetIMAPDialer()
	messages, err := email.PollInbox(context.Background(), cfg, dialer, email.NewMemoryHWMStore())
	if err != nil {
		t.Fatalf("PollInbox failed: %v", err)
	}

	if len(messages) != 1 || messages[0].Subject != "Recent" {
		t.Fatalf("expected only recent message within SinceDays=2, got: %#v", messages)
	}
}

func TestStrictTLSDefaults(t *testing.T) {
	// A server that requires TLS 1.2
	server, err := startFakeIMAPServer(true)
	if err != nil {
		t.Fatalf("startFakeIMAPServer: %v", err)
	}
	defer server.close()

	// Connect without custom TLSConfig -> Should fail certificate validation (since self-signed)
	// confirming that strict verification is enabled by default.
	cfg := email.IMAPConfig{
		Host:     "127.0.0.1",
		Port:     server.port,
		Username: "user",
		Password: "password",
		UseTLS:   true,
		Timeout:  time.Second,
	}

	dialer := email.NewNetIMAPDialer()
	_, err = dialer.Dial(context.Background(), cfg)
	if err == nil {
		t.Fatal("expected TLS certificate verification failure with default strict TLS config, got nil")
	}
	if !strings.Contains(err.Error(), "certificate") && !strings.Contains(err.Error(), "connect/login failed") {
		t.Fatalf("unexpected error: %v", err)
	}
}
