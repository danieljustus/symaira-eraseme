package mcp_test

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

type fakeMCPDialer struct {
	session email.IMAPSession
	dialErr error
	lastCfg email.IMAPConfig
}

func (d *fakeMCPDialer) Dial(_ context.Context, cfg email.IMAPConfig) (email.IMAPSession, error) {
	d.lastCfg = cfg
	if d.dialErr != nil {
		return nil, d.dialErr
	}
	return d.session, nil
}

type fakeMCPSession struct {
	uidValidity uint32
	uids        []uint32
	messages    []email.FetchedMessage
	selected    []string
}

func (s *fakeMCPSession) Select(_ context.Context, folder string) (uint32, error) {
	s.selected = append(s.selected, folder)
	return s.uidValidity, nil
}

func (s *fakeMCPSession) SearchUID(_ context.Context, _ string, _ ...time.Time) ([]uint32, error) {
	return append([]uint32(nil), s.uids...), nil
}

func (s *fakeMCPSession) Fetch(_ context.Context, _ []uint32) ([]email.FetchedMessage, error) {
	return append([]email.FetchedMessage(nil), s.messages...), nil
}

func (s *fakeMCPSession) Close() error {
	return nil
}

func TestMCPContractHandlerPollInboxEndToEnd(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)

	// Setup SQLite DB with a removal request and SENT event
	dbPath := filepath.Join(dataDir, "symeraseme.db")
	store, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("Open store: %v", err)
	}

	reqID, err := store.CreateRemovalRequest(context.Background(), "test-broker", "email", "campaign-1", "US", "tmpl", "hash")
	if err != nil {
		t.Fatalf("CreateRemovalRequest: %v", err)
	}

	_, _, err = store.AppendAndProject(context.Background(), reqID, eventstore.EvtSent, map[string]any{
		"message_id": "<sent-123@symeraseme>",
		"broker_id":  "test-broker",
	}, eventstore.SrcSystem, time.Now().UTC())
	if err != nil {
		t.Fatalf("AppendAndProject: %v", err)
	}
	_ = store.Close()

	// Setup fake session returning a reply referencing <sent-123@symeraseme>
	replyHeader := "Subject: Re: Data Deletion\r\nFrom: broker@example.com\r\nTo: user@example.com\r\nIn-Reply-To: <sent-123@symeraseme>\r\nMessage-ID: <reply-456@broker>\r\n\r\n"
	replyBody := "We have deleted your account data."
	session := &fakeMCPSession{
		uidValidity: 500,
		uids:        []uint32{42},
		messages: []email.FetchedMessage{
			{UID: 42, Header: []byte(replyHeader), Body: []byte(replyBody), InternalDate: time.Now().UTC()},
		},
	}
	dialer := &fakeMCPDialer{session: session}

	handler := mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{IMAPDialer: dialer})
	ctx := context.Background()

	result, err := handler(ctx, "poll_inbox", map[string]any{
		"host":       "imap.testserver.local",
		"port":       993,
		"username":   "testuser",
		"password":   "secretpass",
		"ssl":        true,
		"since_days": 7,
		"folders":    []any{"INBOX"},
	})
	if err != nil {
		t.Fatalf("poll_inbox handler returned error: %v", err)
	}

	resMap, ok := result.(map[string]any)
	if !ok {
		t.Fatalf("expected map[string]any result, got %T: %#v", result, result)
	}

	if resMap["total_fetched"] != 1 {
		t.Fatalf("expected total_fetched = 1, got %v", resMap["total_fetched"])
	}
	if resMap["total_matched"] != 1 {
		t.Fatalf("expected total_matched = 1, got %v", resMap["total_matched"])
	}

	// Verify reply was stored in inbox_replies in SQLite
	storeReopened, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("reopen store: %v", err)
	}
	defer storeReopened.Close()

	var count int
	var storedSnippet string
	var storedReqID int64
	err = storeReopened.DB().QueryRow("SELECT COUNT(*), snippet, request_id FROM inbox_replies WHERE message_id = '<reply-456@broker>'").Scan(&count, &storedSnippet, &storedReqID)
	if err != nil || count != 1 {
		t.Fatalf("expected 1 reply in inbox_replies, got count=%d err=%v", count, err)
	}
	if storedReqID != reqID {
		t.Fatalf("expected stored request_id = %d, got %d", reqID, storedReqID)
	}
	if !strings.Contains(storedSnippet, "deleted your account") {
		t.Fatalf("unexpected stored snippet: %q", storedSnippet)
	}

	// Verify HWM was persisted in imap_state
	v, u, err := storeReopened.GetIMAPHWM(ctx, "imap.testserver.local", "INBOX")
	if err != nil || v == nil || *v != 500 || u == nil || *u != 42 {
		t.Fatalf("expected persisted HWM (500, 42), got v=%v, u=%v, err=%v", v, u, err)
	}
}

func TestMCPContractHandlerConfigPrecedence(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)

	// Set env vars
	t.Setenv("IMAP_HOST", "env.imap.host")
	t.Setenv("IMAP_PORT", "1993")
	t.Setenv("IMAP_USERNAME", "env_user")
	t.Setenv("IMAP_PASSWORD", "env_pass")
	t.Setenv("IMAP_SSL", "true")
	t.Setenv("IMAP_FOLDER", "EnvFolder")
	t.Setenv("IMAP_SINCE_DAYS", "3")

	session := &fakeMCPSession{uidValidity: 1}
	dialer := &fakeMCPDialer{session: session}

	handler := mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{IMAPDialer: dialer})
	ctx := context.Background()

	// 1. When args omit host/port/username, env vars are used
	_, err := handler(ctx, "poll_inbox", map[string]any{})
	if err != nil {
		t.Fatalf("poll_inbox handler failed: %v", err)
	}

	if dialer.lastCfg.Host != "env.imap.host" {
		t.Fatalf("expected Host from env 'env.imap.host', got %q", dialer.lastCfg.Host)
	}
	if dialer.lastCfg.Port != 1993 {
		t.Fatalf("expected Port from env 1993, got %d", dialer.lastCfg.Port)
	}
	if dialer.lastCfg.Username != "env_user" {
		t.Fatalf("expected Username from env 'env_user', got %q", dialer.lastCfg.Username)
	}
	if dialer.lastCfg.Password != "env_pass" {
		t.Fatalf("expected Password from env 'env_pass', got %q", dialer.lastCfg.Password)
	}
	if dialer.lastCfg.SinceDays != 3 {
		t.Fatalf("expected SinceDays from env 3, got %d", dialer.lastCfg.SinceDays)
	}

	// 2. When args specify overrides, args win over env vars
	_, err = handler(ctx, "poll_inbox", map[string]any{
		"host":       "arg.imap.host",
		"port":       2993,
		"username":   "arg_user",
		"password":   "arg_pass",
		"since_days": 14,
	})
	if err != nil {
		t.Fatalf("poll_inbox with args failed: %v", err)
	}

	if dialer.lastCfg.Host != "arg.imap.host" {
		t.Fatalf("expected Host from args 'arg.imap.host', got %q", dialer.lastCfg.Host)
	}
	if dialer.lastCfg.Port != 2993 {
		t.Fatalf("expected Port from args 2993, got %d", dialer.lastCfg.Port)
	}
	if dialer.lastCfg.Username != "arg_user" {
		t.Fatalf("expected Username from args 'arg_user', got %q", dialer.lastCfg.Username)
	}
	if dialer.lastCfg.Password != "arg_pass" {
		t.Fatalf("expected Password from args 'arg_pass', got %q", dialer.lastCfg.Password)
	}
	if dialer.lastCfg.SinceDays != 14 {
		t.Fatalf("expected SinceDays from args 14, got %d", dialer.lastCfg.SinceDays)
	}
}

func TestMCPContractHandlerRedactsSecretsOnError(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)

	secretVal := "SUPER_CONFIDENTIAL_TOKEN_999"
	dialer := &fakeMCPDialer{dialErr: errors.New("connection failed with secret: " + secretVal)}

	handler := mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{IMAPDialer: dialer})
	_, err := handler(context.Background(), "poll_inbox", map[string]any{
		"host":     "imap.example.com",
		"password": secretVal,
	})
	if err == nil {
		t.Fatal("expected error, got nil")
	}

	if strings.Contains(err.Error(), secretVal) {
		t.Fatalf("secret leaked in error: %v", err)
	}
}
